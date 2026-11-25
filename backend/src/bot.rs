use chrono::{DateTime, Duration as ChronoDuration, FixedOffset, NaiveDate, TimeZone, Utc};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use teloxide::{
    prelude::*,
    types::{KeyboardButton, KeyboardMarkup, Message},
};

use crate::{
    services::{eios_sync::EiosSync, schedule_sync},
    state::AppState,
};

fn moscow_offset() -> FixedOffset {
    FixedOffset::east_opt(3 * 60 * 60).expect("UTC+3")
}

fn moscow_time(value: DateTime<Utc>, format: &str) -> String {
    value
        .with_timezone(&moscow_offset())
        .format(format)
        .to_string()
}

fn main_keyboard() -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![
            KeyboardButton::new("📅 Сегодня"),
            KeyboardButton::new("📅 Завтра"),
        ],
        vec![
            KeyboardButton::new("🗓 Неделя"),
            KeyboardButton::new("⏭ Следующая пара"),
        ],
        vec![
            KeyboardButton::new("📚 Задания"),
            KeyboardButton::new("🔔 Напоминания"),
        ],
        vec![
            KeyboardButton::new("⚙️ Настройки"),
            KeyboardButton::new("🏠 Меню"),
        ],
    ])
    .resize_keyboard()
}

fn telegram_user_id(message: &Message) -> Option<i64> {
    message
        .from
        .as_ref()
        .and_then(|user| i64::try_from(user.id.0).ok())
}

async fn ensure_user(db: &PgPool, message: &Message) -> Result<i64, sqlx::Error> {
    let Some(user) = message.from.as_ref() else {
        return Err(sqlx::Error::Protocol(
            "Telegram message has no sender".into(),
        ));
    };

    let telegram_id = i64::try_from(user.id.0)
        .map_err(|_| sqlx::Error::Protocol("Invalid Telegram user ID".into()))?;

    sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO users (telegram_id, first_name)
        VALUES ($1, $2)
        ON CONFLICT (telegram_id)
        DO UPDATE SET
            first_name = EXCLUDED.first_name,
            updated_at = now()
        RETURNING telegram_id
        "#,
    )
    .bind(telegram_id)
    .bind(&user.first_name)
    .fetch_one(db)
    .await
}

async fn reply(bot: &Bot, db: &PgPool, message: &Message, text: String) -> ResponseResult<()> {
    let sent = bot
        .send_message(message.chat.id, text)
        .reply_markup(main_keyboard())
        .await?;

    if let Some(telegram_id) = telegram_user_id(message) {
        let _ = sqlx::query(
            r#"
            INSERT INTO bot_messages (telegram_id, chat_id, message_id)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(telegram_id)
        .bind(message.chat.id.0)
        .bind(sent.id.0)
        .execute(db)
        .await;
    }

    Ok(())
}

async fn find_group(
    db: &PgPool,
    argument: &str,
) -> Result<Option<(uuid::Uuid, String)>, sqlx::Error> {
    sqlx::query_as::<_, (uuid::Uuid, String)>(
        r#"
        SELECT id, name
        FROM groups
        WHERE is_active = true AND name ILIKE $1
        ORDER BY name
        LIMIT 1
        "#,
    )
    .bind(format!("%{}%", argument))
    .fetch_optional(db)
    .await
}

async fn selected_group(db: &PgPool, telegram_id: i64, chat_id: i64) -> Option<uuid::Uuid> {
    if chat_id < 0 {
        sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT group_id FROM bot_chat_settings WHERE chat_id = $1",
        )
        .bind(chat_id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
    } else {
        sqlx::query_scalar::<_, Option<uuid::Uuid>>(
            "SELECT group_id FROM users WHERE telegram_id = $1",
        )
        .bind(telegram_id)
        .fetch_one(db)
        .await
        .ok()
        .flatten()
    }
}


fn period(command: &str) -> (DateTime<Utc>, DateTime<Utc>) {
    let now = Utc::now();
    let local_now = now.with_timezone(&moscow_offset());
    let today: NaiveDate = local_now.date_naive();

    match command {
        "/today" | "📅 Сегодня" => {
            let tomorrow = today + ChronoDuration::days(1);
            let from = moscow_offset()
                .from_local_datetime(&today.and_hms_opt(0, 0, 0).unwrap())
                .single()
                .unwrap()
                .with_timezone(&Utc);
            let to = moscow_offset()
                .from_local_datetime(&tomorrow.and_hms_opt(0, 0, 0).unwrap())
                .single()
                .unwrap()
                .with_timezone(&Utc);
            (from, to)
        }
        "/tomorrow" | "📅 Завтра" => {
            let tomorrow = today + ChronoDuration::days(1);
            let day_after = today + ChronoDuration::days(2);
            let from = moscow_offset()
                .from_local_datetime(&tomorrow.and_hms_opt(0, 0, 0).unwrap())
                .single()
                .unwrap()
                .with_timezone(&Utc);
            let to = moscow_offset()
                .from_local_datetime(&day_after.and_hms_opt(0, 0, 0).unwrap())
                .single()
                .unwrap()
                .with_timezone(&Utc);
            (from, to)
        }
        "/week" | "🗓 Неделя" => (now, now + ChronoDuration::days(7)),
        _ => (now, now + ChronoDuration::days(14)),
    }
}

async fn schedule_text(db: &PgPool, group_id: uuid::Uuid, command: &str) -> String {
    let (from, to) = period(command);

    let rows = sqlx::query_as::<
        _,
        (
            String,
            DateTime<Utc>,
            DateTime<Utc>,
            Option<String>,
            Option<String>,
        ),
    >(
        r#"
        SELECT
            l.subject,
            l.starts_at,
            l.ends_at,
            r.name,
            t.full_name
        FROM lessons l
        LEFT JOIN rooms r ON r.id = l.room_id
        LEFT JOIN teachers t ON t.id = l.teacher_id
        WHERE l.group_id = $1
          AND l.starts_at >= $2
          AND l.starts_at < $3
        ORDER BY l.starts_at
        LIMIT 30
        "#,
    )
    .bind(group_id)
    .bind(from)
    .bind(to)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    if rows.is_empty() {
        return "📭 На этот период занятий не найдено.\nВозможно, расписание ещё загружается."
            .into();
    }

    let selected = if command == "/next" || command == "⏭ Следующая пара" {
        rows.into_iter().take(1).collect::<Vec<_>>()
    } else {
        rows
    };

    let mut output = String::from("🎓 *РАСПИСАНИЕ*\n\n");

    for (subject, start, end, room, teacher) in selected {
        output.push_str(&format!(
            "━━━━━━━━━━━━━━\n📚 *{}*\n🕒 {}–{}\n📍 {}\n👨‍🏫 {}\n",
            subject,
            moscow_time(start, "%d.%m %H:%M"),
            moscow_time(end, "%H:%M"),
            room.unwrap_or_else(|| "Аудитория не указана".into()),
            teacher.unwrap_or_else(|| "Преподаватель не указан".into()),
        ));
    }

    output
}

async fn handle(bot: Bot, state: Arc<AppState>, message: Message) -> ResponseResult<()> {
    let Some(text) = message.text() else {
        return Ok(());
    };
    tracing::info!(
        "Telegram message received: chat_id={}, text={:?}",
        message.chat.id,
        text
    );
    let Some(telegram_id) = telegram_user_id(&message) else {
        return Ok(());
    };

    let db = &state.db;

    if ensure_user(db, &message).await.is_err() {
        reply(
            &bot,
            db,
            &message,
            "Не удалось сохранить профиль. Попробуйте позже.".into(),
        )
        .await?;
        return Ok(());
    }

    let normalized_text = text.trim();

    let command = match normalized_text {
        "📅 Сегодня" => "/today",
        "📅 Завтра" => "/tomorrow",
        "🗓 Неделя" => "/week",
        "⏭ Следующая пара" => "/next",
        "📚 Задания" => "/homework",
        "🔔 Напоминания" => "/reminders",
        "⚙️ Настройки" => "/settings",
        "🏠 Меню" => "/menu",
        other => other
            .splitn(2, char::is_whitespace)
            .next()
            .unwrap_or("")
            .split('@')
            .next()
            .unwrap_or(""),
    };

    let argument = normalized_text
        .splitn(2, char::is_whitespace)
        .nth(1)
        .unwrap_or("")
        .trim();

    let is_group_chat = message.chat.id.0 < 0;

    // Refresh Telegram's authoritative owner/admin/member status only for
    // management commands, avoiding an API request for every ordinary message.
    if is_group_chat && matches!(command, "/settings" | "/setgroup" | "/role") {
        if let Err(error) = crate::services::chat_roles::sync_member(
            &bot,
            db,
            message.chat.id,
            telegram_id,
        )
        .await
        {
            tracing::debug!("Could not synchronize Telegram chat member: {}", error);
        }
    }

    let answer = match command {
        "/start" | "/help" | "/menu" => "🎓 *SMART CAMPUS*\n\n\
            Твой учебный помощник!\n\n\
            📅 /today — расписание на сегодня\n\
            📅 /tomorrow — расписание на завтра\n\
            🗓 /week — расписание на 7 дней\n\
            ⏭ /next — следующая пара\n\
            👥 /group название — выбрать личную группу\n\
            🛠 /setgroup название — настроить группу чата\n\
            📚 /homework — домашние задания\n\
            🔔 /remind_on — включить личные напоминания\n\
            🔕 /remind_off — выключить напоминания\n\
            ⚙️ /settings — настройки\n\n\
            Выбери действие с помощью кнопок ниже."
            .to_string(),

        "/group" => {
            if is_group_chat {
                "В общем чате используй /setgroup название. Изменить группу может администратор чата.".into()
            } else if argument.is_empty() {
                "Укажи название группы: /group ИВТ-21".into()
            } else {
                match find_group(db, argument).await {
                    Ok(Some((group_id, name))) => {
                        let result = sqlx::query(
                            "UPDATE users SET group_id=$2, updated_at=now() WHERE telegram_id=$1",
                        )
                        .bind(telegram_id)
                        .bind(group_id)
                        .execute(db)
                        .await;

                        if result.is_ok() {
                            format!(
                                "✅ Твоя группа: {}\nТеперь доступны расписание и задания.",
                                name
                            )
                        } else {
                            "Не удалось сохранить группу.".into()
                        }
                    }
                    Ok(None) => "Не нашёл такую группу. Проверь название.".into(),
                    Err(_) => "Ошибка при поиске группы.".into(),
                }
            }
        }

        "/setgroup" => {
            if !is_group_chat {
                "Эта команда предназначена для группового чата Telegram.".into()
            } else if !matches!(
                crate::services::chat_roles::sync_member(
                    &bot, db, message.chat.id, telegram_id
                )
                .await
                .ok()
                .flatten()
                .as_deref(),
                Some("owner" | "admin")
            ) {
                "⛔ Менять учебную группу чата может только создатель или администратор Telegram.".into()
            } else if argument.is_empty() {
                "Укажи группу: /setgroup ЛПб-ИСИТ-26-1".into()
            } else {
                match find_group(db, argument).await {
                    Ok(Some((group_id, name))) => {
                        let result = sqlx::query(
                            r#"
                            INSERT INTO bot_chat_settings
                                (chat_id, group_id, configured_by, updated_at)
                            VALUES ($1, $2, $3, now())
                            ON CONFLICT (chat_id)
                            DO UPDATE SET
                                group_id = EXCLUDED.group_id,
                                configured_by = EXCLUDED.configured_by,
                                updated_at = now()
                            "#,
                        )
                        .bind(message.chat.id.0)
                        .bind(group_id)
                        .bind(telegram_id)
                        .execute(db)
                        .await;

                        if result.is_ok() {
                            // Fetch the sender's real Telegram status after the chat
                            // settings row exists; never infer ownership from /setgroup.
                            let _ = crate::services::chat_roles::sync_member(
                                &bot,
                                db,
                                message.chat.id,
                                telegram_id,
                            )
                            .await;
                            format!("✅ Для этого чата установлена группа: {}", name)
                        } else {
                            "Не удалось сохранить настройки чата. Проверь, применена ли миграция."
                                .into()
                        }
                    }
                    Ok(None) => "Не нашёл такую группу. Проверь название.".into(),
                    Err(_) => "Ошибка при поиске группы.".into(),
                }
            }
        }

        "/today" | "/tomorrow" | "/week" | "/next" => {
            match selected_group(db, telegram_id, message.chat.id.0).await {
                Some(group_id) => schedule_text(db, group_id, command).await,
                None if is_group_chat => {
                    "Сначала администратор должен привязать учебную группу:\n/setgroup название"
                        .into()
                }
                None => "Сначала выбери свою группу:\n/group название".into(),
            }
        }

        "/remind_on" | "/remind_off" => {
            let enabled = command == "/remind_on";
            let result = sqlx::query(
                r#"
                UPDATE users
                SET settings = jsonb_set(
                    settings,
                    '{reminders_enabled}',
                    to_jsonb($2::boolean),
                    true
                ),
                updated_at = now()
                WHERE telegram_id = $1
                "#,
            )
            .bind(telegram_id)
            .bind(enabled)
            .execute(db)
            .await;

            if result.is_ok() {
                if enabled {
                    "🔔 Личные напоминания включены. Напишу за 10 минут до пары.".into()
                } else {
                    "🔕 Личные напоминания выключены.".into()
                }
            } else {
                "Не удалось изменить настройки напоминаний.".into()
            }
        }

        "/reminders" => {
            let settings: serde_json::Value =
                sqlx::query_scalar("SELECT settings FROM users WHERE telegram_id=$1")
                    .bind(telegram_id)
                    .fetch_one(db)
                    .await
                    .unwrap_or_default();

            if settings["reminders_enabled"] == true {
                "🔔 Личные напоминания включены.\nДля отключения: /remind_off".into()
            } else {
                "🔕 Личные напоминания выключены.\nДля включения: /remind_on".into()
            }
        }

        "/role" => {
            if !is_group_chat {
                "Команда /role работает только в групповом чате.".into()
            } else {
                let actor_role = crate::services::chat_roles::sync_member(
                    &bot, db, message.chat.id, telegram_id
                )
                .await
                .ok()
                .flatten();

                if !matches!(actor_role.as_deref(), Some("owner" | "admin")) {
                    "⛔ Назначать роли может только создатель чата или администратор Telegram.".into()
                } else {
                    let mut parts = argument.split_whitespace();
                    let target_id = parts.next().and_then(|value| value.parse::<i64>().ok());
                    let requested_role = parts.next().unwrap_or("");

                    let can_assign_senior = actor_role.as_deref() == Some("owner");
                    let valid_role = matches!(requested_role, "moderator" | "helper" | "member")
                        || (can_assign_senior && requested_role == "senior_moderator");

                    if parts.next().is_some() || target_id.is_none() || !valid_role {
                        if can_assign_senior {
                            "Формат: /role TELEGRAM_ID senior_moderator|moderator|helper|member".into()
                        } else {
                            "Администратор Telegram может назначать только moderator, helper или member. Старшего модератора назначает создатель чата.".into()
                        }
                    } else {
                        let target_id = target_id.unwrap();
                        let target_role = crate::services::chat_roles::sync_member(
                            &bot, db, message.chat.id, target_id
                        )
                        .await
                        .ok()
                        .flatten();

                        match target_role {
                            None => "Не удалось проверить участника. Укажи Telegram ID участника, который состоит в этом чате.".into(),
                            Some(ref role) if role == "owner" || role == "admin" => {
                                "Роль владельца и Telegram-администратора нельзя изменить командой бота.".into()
                            }
                            Some(_) => {
                                let result = sqlx::query(
                                    r#"
                                    UPDATE bot_chat_members
                                    SET role = $3, assigned_by = $4, updated_at = now()
                                    WHERE chat_id = $1 AND telegram_id = $2
                                    "#
                                )
                                .bind(message.chat.id.0)
                                .bind(target_id)
                                .bind(requested_role)
                                .bind(telegram_id)
                                .execute(db)
                                .await;

                                if result.is_ok() {
                                    let _ = sqlx::query(
                                        r#"
                                        INSERT INTO bot_chat_audit_logs
                                            (chat_id, actor_telegram_id, target_telegram_id, action, details)
                                        VALUES ($1, $2, $3, 'role_changed', jsonb_build_object('role', $4))
                                        "#
                                    )
                                    .bind(message.chat.id.0)
                                    .bind(telegram_id)
                                    .bind(target_id)
                                    .bind(requested_role)
                                    .execute(db)
                                    .await;
                                    format!("✅ Участнику {} назначена роль: {}", target_id, requested_role)
                                } else {
                                    "Не удалось сохранить роль участника.".into()
                                }
                            }
                        }
                    }
                }
            }
        }

        "/settings" => {
            if is_group_chat {
                let group: Option<String> = sqlx::query_scalar(
                    r#"
                    SELECT g.name
                    FROM bot_chat_settings c
                    JOIN groups g ON g.id = c.group_id
                    WHERE c.chat_id = $1
                    "#,
                )
                .bind(message.chat.id.0)
                .fetch_optional(db)
                .await
                .unwrap_or(None);

                format!(
                    "⚙️ Настройки чата\nУчебная группа: {}\nИзменение: /setgroup название",
                    group.unwrap_or_else(|| "не установлена".into())
                )
            } else {
                let group: Option<String> = sqlx::query_scalar(
                    r#"
                    SELECT g.name
                    FROM users u
                    LEFT JOIN groups g ON g.id = u.group_id
                    WHERE u.telegram_id = $1
                    "#,
                )
                .bind(telegram_id)
                .fetch_one(db)
                .await
                .unwrap_or(None);

                let settings: serde_json::Value =
                    sqlx::query_scalar("SELECT settings FROM users WHERE telegram_id=$1")
                        .bind(telegram_id)
                        .fetch_one(db)
                        .await
                        .unwrap_or_default();

                format!(
                    "⚙️ Твои настройки\nГруппа: {}\nНапоминания: {}",
                    group.unwrap_or_else(|| "не выбрана".into()),
                    if settings["reminders_enabled"] == true {
                        "включены"
                    } else {
                        "выключены"
                    }
                )
            }
        }

        "/homework" => {
            let group_id = selected_group(db, telegram_id, message.chat.id.0).await;

            if let Some(group_id) = group_id {
                if argument.is_empty() {
                    let tasks = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
                        r#"
                        SELECT title, due_at
                        FROM group_tasks
                        WHERE group_id = $1
                        ORDER BY due_at NULLS LAST, created_at DESC
                        LIMIT 15
                        "#,
                    )
                    .bind(group_id)
                    .fetch_all(db)
                    .await
                    .unwrap_or_default();

                    if tasks.is_empty() {
                        "📚 Пока нет добавленных заданий.".into()
                    } else {
                        let mut output = String::from("📚 *ЗАДАНИЯ ГРУППЫ*\n\n");
                        for (title, due_at) in tasks {
                            let due = due_at
                                .map(|date| moscow_time(date, "%d.%m %H:%M"))
                                .unwrap_or_else(|| "срок не указан".into());
                            output.push_str(&format!("• {}\n  Срок: {}\n\n", title, due));
                        }
                        output
                    }
                } else if is_group_chat {
                    "Добавлять задания через общий чат пока нельзя. Используй личный чат с ботом."
                        .into()
                } else {
                    let result = sqlx::query(
                        r#"
                        INSERT INTO group_tasks (group_id, title, created_by)
                        SELECT $1, $2, id
                        FROM users
                        WHERE telegram_id = $3
                        "#,
                    )
                    .bind(group_id)
                    .bind(argument)
                    .bind(telegram_id)
                    .execute(db)
                    .await;

                    if result.is_ok() {
                        "✅ Задание добавлено в список группы.".into()
                    } else {
                        "Не удалось добавить задание.".into()
                    }
                }
            } else if is_group_chat {
                "Сначала настрой группу чата командой /setgroup название.".into()
            } else {
                "Сначала выбери группу: /group название".into()
            }
        }

        "/clear" => {
            if is_group_chat {
                "Очистку сообщений запускай в личном чате с ботом.".into()
            } else {
                let ids: Vec<i32> = sqlx::query_scalar(
                    r#"
                    DELETE FROM bot_messages
                    WHERE telegram_id = $1
                      AND id IN (
                        SELECT id
                        FROM bot_messages
                        WHERE telegram_id = $1
                        ORDER BY created_at DESC
                        LIMIT 30
                      )
                    RETURNING message_id
                    "#,
                )
                .bind(telegram_id)
                .fetch_all(db)
                .await
                .unwrap_or_default();

                let mut removed = 0;
                for id in ids {
                    if bot
                        .delete_message(message.chat.id, teloxide::types::MessageId(id))
                        .await
                        .is_ok()
                    {
                        removed += 1;
                    }
                }
                format!("🧹 Удалено сообщений бота: {}", removed)
            }
        }
        "/sync" => {
            if is_group_chat
                || state.config.admin_telegram_id <= 0
                || telegram_id != state.config.admin_telegram_id
            {
                "⛔ Команда доступна только администратору в личном чате с ботом.".into()
            } else {
                let sync_bot = bot.clone();
                let sync_chat = message.chat.id;
                let sync_db = db.clone();

                tokio::spawn(async move {
                    let result = match EiosSync::new("data/eios_cache", "1") {
                        Ok(eios) => schedule_sync::sync_all_groups(&sync_db, &eios).await,
                        Err(error) => Err(error.to_string()),
                    };

                    let response = match result {
                Ok(()) => "✅ Синхронизация расписаний завершена. Подробности есть в логах сервера.".to_owned(),
                Err(error) => format!("❌ Синхронизация не выполнена: {}", error),
            };

                    if let Err(error) = sync_bot.send_message(sync_chat, response).await {
                        tracing::error!("Could not send sync result to admin: {}", error);
                    }
                });

                "⏳ Запустил синхронизацию расписаний. Это может занять несколько минут. Напишу, когда закончу.".into()
            }
        }
        _ => return Ok(()),
    };

    reply(&bot, db, &message, answer).await?;
    Ok(())
}

pub async fn run(state: Arc<AppState>) {
    let Some(token) = state.config.teloxide_token.clone() else {
        tracing::warn!("TELOXIDE_TOKEN is not configured; bot is disabled");
        return;
    };

    let bot = Bot::new(token);
    let reminder_bot = bot.clone();
    let reminder_state = state.clone();

    tokio::spawn(async move {
        reminder_loop(reminder_bot, reminder_state).await;
    });

    let handler = Update::filter_message().endpoint(move |bot: Bot, message: Message| {
        let state = state.clone();
        async move { handle(bot, state, message).await }
    });

    Dispatcher::builder(bot, handler).build().dispatch().await;
}

async fn reminder_loop(bot: Bot, state: Arc<AppState>) {
    let mut tick = tokio::time::interval(Duration::from_secs(30));

    loop {
        tick.tick().await;

        let rows = sqlx::query_as::<_, (i64, String, String, DateTime<Utc>)>(
            r#"
            SELECT
                u.telegram_id,
                l.id::text,
                l.subject,
                l.starts_at
            FROM users u
            JOIN lessons l ON l.group_id = u.group_id
            WHERE COALESCE(
                (u.settings->>'reminders_enabled')::boolean,
                false
            ) = true
              AND l.starts_at BETWEEN now() + interval '9 minutes'
                                  AND now() + interval '10 minutes'
              AND NOT EXISTS (
                SELECT 1
                FROM bot_reminder_deliveries d
                WHERE d.telegram_id = u.telegram_id
                  AND d.lesson_id = l.id
              )
            "#,
        )
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        for (telegram_id, lesson_id, subject, starts_at) in rows {
            if let Ok(id) = uuid::Uuid::parse_str(&lesson_id) {
                let inserted = sqlx::query(
                    r#"
                    INSERT INTO bot_reminder_deliveries (telegram_id, lesson_id)
                    VALUES ($1, $2)
                    ON CONFLICT DO NOTHING
                    "#,
                )
                .bind(telegram_id)
                .bind(id)
                .execute(&state.db)
                .await;

                if inserted.is_ok() {
                    let _ = bot
                        .send_message(
                            ChatId(telegram_id),
                            format!(
                                "⏰ Через 10 минут пара: {}\nНачало: {}",
                                subject,
                                moscow_time(starts_at, "%H:%M")
                            ),
                        )
                        .await;
                }
            }
        }
    }
}
