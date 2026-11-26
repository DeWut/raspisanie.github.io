use chrono::{DateTime, Duration as ChronoDuration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use teloxide::{
    prelude::*,
    types::{
        BotCommand, BotCommandScope, ButtonRequest, ChatPermissions, InlineQuery,
        InlineQueryResult, InlineQueryResultArticle, InputMessageContent, InputMessageContentText,
        KeyboardButton, KeyboardMarkup, Message, MessageId, ThreadId, UserId, WebAppInfo,
    },
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

fn main_keyboard(webapp_url: &str, is_private: bool) -> KeyboardMarkup {
    let mut rows = vec![
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
    ];

    if is_private {
        if let Ok(url) = url::Url::parse(webapp_url) {
            if url.scheme() == "https" && url.host_str() != Some("example.com") {
                rows.push(vec![KeyboardButton::new("🌲 Открыть SPBFTUIT")
                    .request(ButtonRequest::WebApp(WebAppInfo { url }))]);
            }
        }
    }

    KeyboardMarkup::new(rows).resize_keyboard()
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

async fn reply(
    bot: &Bot,
    db: &PgPool,
    message: &Message,
    text: String,
    webapp_url: &str,
) -> ResponseResult<()> {
    let sent = bot
        .send_message(message.chat.id, text)
        .reply_markup(main_keyboard(webapp_url, message.chat.is_private()))
        .await?;

    if let Some(telegram_id) = telegram_user_id(message) {
        let result = async {
            let mut tx = db.begin().await?;
            sqlx::query(
                r#"
                INSERT INTO bot_messages (telegram_id, chat_id, message_id)
                VALUES ($1, $2, $3)
                "#,
            )
            .bind(telegram_id)
            .bind(message.chat.id.0)
            .bind(sent.id.0)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                DELETE FROM bot_messages
                WHERE telegram_id = $1 AND chat_id = $2
                  AND id NOT IN (
                    SELECT id FROM bot_messages
                    WHERE telegram_id = $1 AND chat_id = $2
                    ORDER BY id DESC
                    LIMIT 30
                  )
                "#,
            )
            .bind(telegram_id)
            .bind(message.chat.id.0)
            .execute(&mut *tx)
            .await?;

            tx.commit().await
        }
        .await;

        if result.is_err() {
            tracing::warn!("Could not update bot message tracking");
        }
    }

    Ok(())
}

fn parse_target_and_tail(message: &Message, argument: &str) -> Result<(i64, String), String> {
    if let Some(user) = message
        .reply_to_message()
        .and_then(|replied| replied.from.as_ref())
    {
        let target_id = i64::try_from(user.id.0)
            .map_err(|_| "Некорректный Telegram ID участника".to_owned())?;
        return Ok((target_id, argument.trim().to_owned()));
    }

    let mut parts = argument.splitn(2, char::is_whitespace);
    let target_id = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|id| *id > 0)
        .ok_or_else(|| {
            "Ответь на сообщение участника или укажи Telegram ID: /команда ID".to_owned()
        })?;
    Ok((
        target_id,
        parts.next().unwrap_or_default().trim().to_owned(),
    ))
}

fn parse_mute_duration(value: &str) -> Option<ChronoDuration> {
    if !value.is_ascii() {
        return None;
    }
    let (amount, unit) = value.split_at(value.len().checked_sub(1)?);
    let amount = amount.parse::<i64>().ok()?;
    if amount <= 0 {
        return None;
    }

    let maximum = match unit {
        "m" => 30 * 24 * 60,
        "h" => 30 * 24,
        "d" => 30,
        _ => return None,
    };
    if amount > maximum {
        return None;
    }

    let duration = match unit {
        "m" => ChronoDuration::minutes(amount),
        "h" => ChronoDuration::hours(amount),
        "d" => ChronoDuration::days(amount),
        _ => unreachable!("unit was validated above"),
    };
    (duration >= ChronoDuration::minutes(1) && duration <= ChronoDuration::days(30))
        .then_some(duration)
}

async fn record_moderation_event(
    db: &PgPool,
    chat_id: ChatId,
    actor_id: i64,
    target_id: i64,
    action: &str,
    details: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO bot_chat_audit_logs
            (chat_id, actor_telegram_id, target_telegram_id, action, details)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(chat_id.0)
    .bind(actor_id)
    .bind(target_id)
    .bind(action)
    .bind(details)
    .execute(db)
    .await?;
    Ok(())
}

async fn moderation_target_status(
    bot: &Bot,
    db: &PgPool,
    chat_id: ChatId,
    target_id: i64,
) -> Result<(), String> {
    match crate::services::chat_roles::sync_member(bot, db, chat_id, target_id, None).await {
        Ok(Some(role)) if role == "owner" || role == "admin" => {
            Err("Владельца и администратора Telegram нельзя модерировать командой бота.".into())
        }
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            Err("Участник должен состоять в этом чате и не иметь активных ограничений.".into())
        }
        Err(_) => Err("Не удалось проверить участника в Telegram. Попробуй ещё раз.".into()),
    }
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
        let message = match command {
            "/today" | "📅 Сегодня" => {
                "📭 *РАСПИСАНИЕ НА СЕГОДНЯ*\n\nНа сегодня занятий не найдено."
            }
            "/tomorrow" | "📅 Завтра" => {
                "📭 *РАСПИСАНИЕ НА ЗАВТРА*\n\nНа завтра занятий не найдено."
            }
            "/next" | "⏭ Следующая пара" => {
                "⏭ *СЛЕДУЮЩАЯ ПАРА*\n\nНа ближайшие 14 дней занятий не найдено."
            }
            "/week" | "🗓 Неделя" => {
                "📭 *РАСПИСАНИЕ НА 7 ДНЕЙ*\n\nНа ближайшие семь дней занятий не найдено."
            }
            _ => "📭 *РАСПИСАНИЕ*\n\nНа выбранный период занятий не найдено.",
        };
        return format!("{}\nВозможно, расписание ещё не обновилось.", message);
    }

    let is_next = command == "/next" || command == "⏭ Следующая пара";
    let selected = if is_next {
        rows.into_iter().take(1).collect::<Vec<_>>()
    } else {
        rows
    };

    let heading = match command {
        "/today" | "📅 Сегодня" => "📅 *РАСПИСАНИЕ НА СЕГОДНЯ*",
        "/tomorrow" | "📅 Завтра" => "📅 *РАСПИСАНИЕ НА ЗАВТРА*",
        "/next" | "⏭ Следующая пара" => "⏭ *СЛЕДУЮЩАЯ ПАРА*",
        "/week" | "🗓 Неделя" => "🗓 *РАСПИСАНИЕ НА 7 ДНЕЙ*",
        _ => "🎓 *РАСПИСАНИЕ*",
    };
    let mut output = format!("{}\n\n", heading);

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

#[derive(Clone, Copy)]
struct InlineScheduleAction {
    id: &'static str,
    command: Option<&'static str>,
    title: &'static str,
    description: &'static str,
}

const INLINE_SCHEDULE_ACTIONS: [InlineScheduleAction; 4] = [
    InlineScheduleAction {
        id: "inline_today",
        command: Some("/today"),
        title: "📅 Расписание на сегодня",
        description: "Все пары сегодня: время, аудитории и преподаватели",
    },
    InlineScheduleAction {
        id: "inline_tomorrow",
        command: Some("/tomorrow"),
        title: "📅 Расписание на завтра",
        description: "Все пары завтра: время, аудитории и преподаватели",
    },
    InlineScheduleAction {
        id: "inline_next",
        command: Some("/next"),
        title: "⏭ Следующая пара — ближайшее занятие",
        description: "Ближайшая предстоящая пара: предмет, дата, время и аудитория",
    },
    InlineScheduleAction {
        id: "inline_week",
        command: Some("/week"),
        title: "🗓 Расписание на неделю",
        description: "Все занятия с сегодняшнего дня на следующие 7 дней",
    },
];

const INLINE_HELP_ACTION: InlineScheduleAction = InlineScheduleAction {
    id: "inline_help",
    command: None,
    title: "Как искать расписание",
    description: "Запросы: today, tomorrow, next, week",
};

fn inline_actions(query: &str) -> Vec<InlineScheduleAction> {
    match query.trim().to_lowercase().as_str() {
        "" => INLINE_SCHEDULE_ACTIONS.to_vec(),
        "today" | "сегодня" | "/today" => vec![INLINE_SCHEDULE_ACTIONS[0]],
        "tomorrow" | "завтра" | "/tomorrow" => vec![INLINE_SCHEDULE_ACTIONS[1]],
        "next" | "пара" | "следующая" | "следующая пара" | "ближайшая пара"
        | "следующую" | "/next" => vec![INLINE_SCHEDULE_ACTIONS[2]],
        "week" | "неделя" | "7 дней" | "/week" => vec![INLINE_SCHEDULE_ACTIONS[3]],
        "help" | "помощь" | "команды" | "?" => vec![INLINE_HELP_ACTION],
        _ => vec![INLINE_HELP_ACTION],
    }
}

async fn handle_inline_query(
    bot: Bot,
    state: Arc<AppState>,
    query: InlineQuery,
) -> ResponseResult<()> {
    let telegram_id = i64::try_from(query.from.id.0).ok();
    let group_id = match telegram_id {
        Some(id) => selected_group(&state.db, id, id).await,
        None => None,
    };

    let mut results = Vec::<InlineQueryResult>::new();

    if let Some(group_id) = group_id {
        for action in inline_actions(&query.query) {
            let message_text = match action.command {
                Some(command) => schedule_text(&state.db, group_id, command)
                    .await
                    .replace('*', ""),
                None => "Быстрый поиск расписания SPBFTUIT.\n\n\
                    Введи один из запросов после имени бота:\n\
                    today — сегодня\n\
                    tomorrow — завтра\n\
                    next — следующая пара\n\
                    week — ближайшие 7 дней"
                    .to_owned(),
            };

            let article = InlineQueryResultArticle::new(
                action.id.to_owned(),
                action.title.to_owned(),
                InputMessageContent::Text(InputMessageContentText::new(message_text)),
            )
            .description(action.description.to_owned());

            results.push(article.into());
        }
    } else {
        let article = InlineQueryResultArticle::new(
            "inline_choose_group".to_owned(),
            "⚙️ Сначала выбери учебную группу".to_owned(),
            InputMessageContent::Text(InputMessageContentText::new(
                "Чтобы отправлять расписание в чаты, сначала открой @spbftuITbot в личных сообщениях и выбери группу командой /group название. Затем вернись в чат и повтори inline-запрос.",
            )),
        )
        .description("Настрой личную группу в боте, затем используй today, tomorrow, next или week");

        results.push(article.into());
    }

    bot.answer_inline_query(query.id, results)
        .cache_time(0)
        .is_personal(true)
        .await?;

    Ok(())
}

async fn handle(bot: Bot, state: Arc<AppState>, message: Message) -> ResponseResult<()> {
    if let Some(web_app_data) = message.web_app_data() {
        if !message.chat.is_private() || web_app_data.data.len() > 1_000 {
            return Ok(());
        }

        let Some(telegram_id) = telegram_user_id(&message) else {
            return Ok(());
        };
        if message.chat.id.0 != telegram_id {
            return Ok(());
        }

        #[derive(serde::Deserialize)]
        struct MiniAppAction {
            #[serde(rename = "type")]
            action_type: String,
            group_id: String,
        }

        let selected = serde_json::from_str::<MiniAppAction>(&web_app_data.data)
            .ok()
            .filter(|action| action.action_type == "set_personal_group")
            .filter(|action| {
                !action.group_id.is_empty()
                    && action.group_id.len() <= 32
                    && action.group_id.bytes().all(|byte| byte.is_ascii_digit())
            });

        let Some(action) = selected else {
            return Ok(());
        };

        let answer = match ensure_user(&state.db, &message).await {
            Err(_) => "Не удалось открыть профиль. Попробуй ещё раз через /start.".to_owned(),
            Ok(_) => {
                let group = sqlx::query_as::<_, (uuid::Uuid, String)>(
                    "SELECT id, name FROM groups WHERE external_id = $1 AND is_active = TRUE",
                )
                .bind(&action.group_id)
                .fetch_optional(&state.db)
                .await;

                match group {
                    Ok(Some((group_id, name))) => {
                        let saved = sqlx::query(
                            "UPDATE users SET group_id = $2, updated_at = now() WHERE telegram_id = $1",
                        )
                        .bind(telegram_id)
                        .bind(group_id)
                        .execute(&state.db)
                        .await;
                        match saved {
                            Ok(result) if result.rows_affected() == 1 => format!(
                                "✅ Группа {} сохранена в боте. Расписание и напоминания будут использовать её.",
                                name
                            ),
                            _ => "Не удалось сохранить группу. Попробуй открыть бота и отправить /start, затем повтори попытку.".into(),
                        }
                    }
                    Ok(None) => "Эта группа пока недоступна в расписании бота. Попробуй обновить список позже.".into(),
                    Err(_) => "Не удалось сохранить группу из-за ошибки базы данных.".into(),
                }
            }
        };

        reply(&bot, &state.db, &message, answer, &state.config.webapp_url).await?;
        return Ok(());
    }

    let Some(text) = message.text() else {
        return Ok(());
    };
    let Some(telegram_id) = telegram_user_id(&message) else {
        // Anonymous administrators and sender-chat messages have no stable
        // Telegram user ID, so they cannot be authorized for SPBFTUIT roles.
        if message.chat.id.0 < 0 && text.trim_start().starts_with("/role") {
            bot.send_message(
                message.chat.id,
                "Для управления ролями используй личный аккаунт Telegram: анонимного администратора нельзя проверить как участника.",
            )
            .await?;
        }
        return Ok(());
    };

    let db = &state.db;

    if ensure_user(db, &message).await.is_err() {
        reply(
            &bot,
            db,
            &message,
            "Не удалось сохранить профиль. Попробуйте позже.".into(),
            &state.config.webapp_url,
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

    if is_group_chat {
        let username = message
            .from
            .as_ref()
            .and_then(|user| user.username.as_deref());
        if let Err(error) = crate::services::chat_roles::remember_member_identity(
            db,
            message.chat.id,
            telegram_id,
            username,
        )
        .await
        {
            tracing::debug!("Could not cache Telegram chat member identity: {}", error);
        }
    }

    // Refresh Telegram's authoritative owner/admin/member status only for
    // management commands, avoiding an API request for every ordinary message.
    if is_group_chat && matches!(command, "/settings" | "/setgroup" | "/role") {
        if let Err(error) =
            crate::services::chat_roles::sync_member(&bot, db, message.chat.id, telegram_id, None)
                .await
        {
            tracing::debug!("Could not synchronize Telegram chat member: {}", error);
        }
    }

    let answer = match command {
        "/start" | "/help" | "/menu" => {
            let mut help = "🎓 *SPBFTUIT*\n\n\
            Твой учебный помощник!\n\n\
            📅 /today — расписание на сегодня\n\
            📅 /tomorrow — расписание на завтра\n\
            🗓 /week — расписание на 7 дней\n\
            ⏭ /next — следующая пара\n\
            👥 /group название — выбрать личную группу\n\
            🛠 /setgroup название — настроить группу чата\n\
            👮 В группе: /role @username moderator\n\
            ↩️ Ответом на сообщение: /role moderator\n\
            📚 /homework — домашние задания\n\
            🔔 /remind_on — включить личные напоминания\n\
            🔕 /remind_off — выключить напоминания\n\
            ⚙️ /settings — настройки\n\n\
            Выбери действие с помощью кнопок ниже."
                .to_owned();
            if is_group_chat {
                help.push_str(
                    "\n\n📢 *Автоматизация учебного чата*\n/chat_settings — настройки\n/daily_on — расписание каждый день\n/daily_time 07:30 — время публикации (МСК)\n/chat_notify_on — уведомлять за 10 минут до пары\n/daily_off и /chat_notify_off — выключить функции\n\n🛡 *Модераторам*\nОтветом: /warn причина · /mute 30m причина · /unmute · /ban\nПо ID: /warnings ID · /unban ID",
                );
            }
            if !is_group_chat
                && state.config.admin_telegram_id > 0
                && telegram_id == state.config.admin_telegram_id
            {
                help.push_str("\n\n🔄 /sync — вручную обновить расписание из ЭИОС");
            }
            help.push_str("\n\n⚡ Inline-поиск: @spbftuITbot today | tomorrow | next | week");
            help
        }

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
                    &bot,
                    db,
                    message.chat.id,
                    telegram_id,
                    None
                )
                .await
                .ok()
                .flatten()
                .as_deref(),
                Some("owner" | "admin")
            ) {
                "⛔ Менять учебную группу чата может только создатель или администратор Telegram."
                    .into()
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
                                None,
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
                    &bot,
                    db,
                    message.chat.id,
                    telegram_id,
                    None,
                )
                .await
                .ok()
                .flatten();

                if !matches!(actor_role.as_deref(), Some("owner" | "admin")) {
                    "⛔ Назначать роли может только создатель чата или администратор Telegram."
                        .into()
                } else {
                    let mut parts = argument.split_whitespace();
                    let first = parts.next().unwrap_or("");
                    let second = parts.next();
                    let has_extra = parts.next().is_some();
                    let reply_user = message
                        .reply_to_message()
                        .and_then(|replied| replied.from.as_ref());

                    let parsed_target: Result<(i64, Option<String>, String), String> = if first
                        .starts_with('@')
                    {
                        let username = first.trim_start_matches('@');
                        let valid_username = (5..=32).contains(&username.len())
                            && username
                                .chars()
                                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');

                        if !valid_username || second.is_none() || has_extra {
                            Err("Формат: /role @username moderator".into())
                        } else {
                            match crate::services::chat_roles::find_member_by_username(
                                    db,
                                    message.chat.id,
                                    username,
                                )
                                .await
                                {
                                    Ok(Some(target_id)) => Ok((
                                        target_id,
                                        Some(username.to_owned()),
                                        second.unwrap().to_owned(),
                                    )),
                                    Ok(None) => Err(format!(
                                        "Не нашёл @{} среди известных участников этого чата. Попроси человека написать сообщение в чат или используй ответ на его сообщение: /role moderator.",
                                        username
                                    )),
                                    Err(error) => {
                                        tracing::warn!("Could not resolve chat username: {}", error);
                                        Err("Не удалось найти участника по username.".into())
                                    }
                                }
                        }
                    } else if let Ok(target_id) = first.parse::<i64>() {
                        if target_id <= 0 || second.is_none() || has_extra {
                            Err("Формат: /role TELEGRAM_ID moderator".into())
                        } else {
                            Ok((target_id, None, second.unwrap().to_owned()))
                        }
                    } else if let Some(target_user) = reply_user {
                        let target_id = i64::try_from(target_user.id.0)
                            .map_err(|_| "Некорректный Telegram ID участника".to_owned());
                        if second.is_some() || has_extra {
                            Err("При ответе на сообщение используй только: /role moderator".into())
                        } else {
                            target_id.map(|id| (id, None, first.to_owned()))
                        }
                    } else {
                        Err("Укажи @username и роль либо ответь на сообщение участника: /role @username moderator или /role moderator".into())
                    };

                    match parsed_target {
                        Err(error) => error,
                        Ok((target_id, expected_username, requested_role)) => {
                            let can_assign_senior = actor_role.as_deref() == Some("owner");
                            let valid_role = matches!(
                                requested_role.as_str(),
                                "moderator" | "helper" | "member"
                            ) || (can_assign_senior
                                && requested_role == "senior_moderator");

                            if !valid_role {
                                if can_assign_senior {
                                    "Допустимые роли: senior_moderator, moderator, helper, member."
                                        .into()
                                } else {
                                    "Администратор Telegram может назначать только moderator, helper или member. Старшего модератора назначает создатель чата.".into()
                                }
                            } else {
                                match crate::services::chat_roles::sync_member(
                                    &bot,
                                    db,
                                    message.chat.id,
                                    target_id,
                                    expected_username.as_deref(),
                                )
                                .await
                                {
                                    Err(error) => {
                                        tracing::warn!("Could not verify role target: {}", error);
                                        "Не удалось проверить участника в этом чате. Попробуй ответить на его сообщение.".into()
                                    }
                                    Ok(None) => {
                                        "Не удалось проверить участника. Он должен состоять в этом чате.".into()
                                    }
                                    Ok(Some(ref role)) if role == "owner" || role == "admin" => {
                                        "Роль владельца и Telegram-администратора нельзя изменить командой бота.".into()
                                    }
                                    Ok(Some(_)) => {
                                        let result = async {
                                            let mut tx = db.begin().await?;

                                            sqlx::query(
                                                r#"
                                                UPDATE bot_chat_members
                                                SET role = $3, assigned_by = $4, updated_at = now()
                                                WHERE chat_id = $1 AND telegram_id = $2
                                                "#,
                                            )
                                            .bind(message.chat.id.0)
                                            .bind(target_id)
                                            .bind(&requested_role)
                                            .bind(telegram_id)
                                            .execute(&mut *tx)
                                            .await?;

                                            sqlx::query(
                                                r#"
                                                INSERT INTO bot_chat_audit_logs
                                                    (chat_id, actor_telegram_id, target_telegram_id, action, details)
                                                VALUES ($1, $2, $3, 'role_changed', jsonb_build_object('role', $4))
                                                "#,
                                            )
                                            .bind(message.chat.id.0)
                                            .bind(telegram_id)
                                            .bind(target_id)
                                            .bind(&requested_role)
                                            .execute(&mut *tx)
                                            .await?;

                                            tx.commit().await
                                        }
                                        .await;

                                        if result.is_ok() {
                                            format!(
                                                "✅ Участнику {} назначена роль: {}",
                                                target_id, requested_role
                                            )
                                        } else {
                                            tracing::error!("Could not save role change and audit log");
                                            "Не удалось сохранить роль участника.".into()
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        "/warn" => {
            if !is_group_chat {
                "Предупреждения работают только в групповом чате.".into()
            } else if !crate::services::chat_roles::can_manage_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Предупреждения могут выдавать модераторы чата.".into()
            } else {
                match parse_target_and_tail(&message, argument) {
                    Err(error) => error,
                    Ok((target_id, _)) if target_id == telegram_id => {
                        "Нельзя выдать предупреждение самому себе.".into()
                    }
                    Ok((_, ref reason)) if reason.trim().is_empty() => {
                        "Добавь причину: ответь на сообщение командой /warn причина.".into()
                    }
                    Ok((_, ref reason)) if reason.chars().count() > 200 => {
                        "Причина должна быть не длиннее 200 символов.".into()
                    }
                    Ok((target_id, reason)) => {
                        match moderation_target_status(&bot, db, message.chat.id, target_id).await {
                            Err(error) => error,
                            Ok(_) => {
                                let saved = record_moderation_event(
                                    db,
                                    message.chat.id,
                                    telegram_id,
                                    target_id,
                                    "warning",
                                    serde_json::json!({"reason": reason.trim()}),
                                )
                                .await;
                                if saved.is_err() {
                                    "Не удалось сохранить предупреждение в журнал.".into()
                                } else {
                                    let count = sqlx::query_scalar::<_, i64>(
                                        r#"
                                        SELECT count(*)
                                        FROM bot_chat_audit_logs
                                        WHERE chat_id = $1 AND target_telegram_id = $2
                                          AND action = 'warning'
                                        "#,
                                    )
                                    .bind(message.chat.id.0)
                                    .bind(target_id)
                                    .fetch_one(db)
                                    .await;
                                    match count {
                                        Ok(count) => format!(
                                            "⚠️ Участнику {} выдано предупреждение. Всего предупреждений: {}. Причина: {}",
                                            target_id, count, reason.trim()
                                        ),
                                        Err(_) => {
                                            tracing::warn!("Could not count warnings after saving one");
                                            format!("⚠️ Предупреждение участнику {} сохранено. Причина: {}", target_id, reason.trim())
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        "/warnings" => {
            if !is_group_chat {
                "Список предупреждений доступен только в групповом чате.".into()
            } else if !crate::services::chat_roles::can_manage_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Смотреть предупреждения могут модераторы чата.".into()
            } else {
                match parse_target_and_tail(&message, argument) {
                    Err(error) => error,
                    Ok((_, ref tail)) if !tail.is_empty() => {
                        "Ответь на сообщение участника командой /warnings или укажи /warnings TELEGRAM_ID.".into()
                    }
                    Ok((target_id, _)) => {
                        let count = sqlx::query_scalar::<_, i64>(
                            r#"
                            SELECT count(*)
                            FROM bot_chat_audit_logs
                            WHERE chat_id = $1 AND target_telegram_id = $2
                              AND action = 'warning'
                            "#,
                        )
                        .bind(message.chat.id.0)
                        .bind(target_id)
                        .fetch_one(db)
                        .await;
                        match count {
                            Ok(count) => format!("У участника {} предупреждений: {}.", target_id, count),
                            Err(_) => "Не удалось загрузить счётчик предупреждений.".into(),
                        }
                    }
                }
            }
        }

        "/mute" => {
            if !is_group_chat {
                "Мут работает только в групповом чате.".into()
            } else if !message.chat.is_supergroup() {
                "Для временного ограничения нужна супергруппа Telegram.".into()
            } else if !crate::services::chat_roles::can_manage_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Ограничивать участников могут модераторы чата.".into()
            } else {
                match parse_target_and_tail(&message, argument) {
                    Err(error) => error,
                    Ok((target_id, _)) if target_id == telegram_id => {
                        "Нельзя ограничить самого себя.".into()
                    }
                    Ok((target_id, tail)) => {
                        let mut parts = tail.splitn(2, char::is_whitespace);
                        let duration_token = parts.next().unwrap_or_default();
                        let reason = parts.next().unwrap_or_default().trim();
                        match parse_mute_duration(duration_token) {
                            None => "Укажи срок от 1 минуты до 30 дней: /mute 30m причина (ответом на сообщение).".into(),
                            Some(_) if reason.chars().count() > 200 => {
                                "Причина должна быть не длиннее 200 символов.".into()
                            }
                            Some(duration) => {
                                match moderation_target_status(&bot, db, message.chat.id, target_id).await {
                                    Err(error) => error,
                                    Ok(_) => {
                                        let original_permissions = bot
                                            .get_chat(message.chat.id)
                                            .await
                                            .ok()
                                            .and_then(|chat| chat.permissions());
                                        match original_permissions {
                                            None => "Не удалось прочитать права чата; ограничение не применено.".into(),
                                            Some(permissions) => {
                                                let permissions_json = match serde_json::to_value(permissions) {
                                                    Ok(value) => value,
                                                    Err(_) => return reply(
                                                        &bot,
                                                        db,
                                                        &message,
                                                        "Не удалось подготовить права для сохранения; мут не применён.".into(),
                                                        &state.config.webapp_url,
                                                    ).await,
                                                };
                                                let until = Utc::now() + duration;
                                                let stored = sqlx::query(
                                                    r#"
                                                    INSERT INTO bot_chat_mutes
                                                        (chat_id, target_telegram_id, actor_telegram_id,
                                                         original_permissions, muted_until, reason)
                                                    VALUES ($1, $2, $3, $4, $5, $6)
                                                    ON CONFLICT (chat_id, target_telegram_id) DO UPDATE SET
                                                        actor_telegram_id = EXCLUDED.actor_telegram_id,
                                                        original_permissions = EXCLUDED.original_permissions,
                                                        muted_until = EXCLUDED.muted_until,
                                                        reason = EXCLUDED.reason,
                                                        created_at = now()
                                                    WHERE bot_chat_mutes.muted_until <= now()
                                                    "#,
                                                )
                                                .bind(message.chat.id.0)
                                                .bind(target_id)
                                                .bind(telegram_id)
                                                .bind(permissions_json)
                                                .bind(until)
                                                .bind(if reason.is_empty() { None } else { Some(reason) })
                                                .execute(db)
                                                .await;
                                                match stored {
                                                    Ok(result) if result.rows_affected() == 0 => {
                                                        "У участника уже есть активное ограничение SPBFTUIT.".into()
                                                    }
                                                    Err(_) => "Не удалось сохранить данные ограничения.".into(),
                                                    Ok(_) => {
                                                        let details = serde_json::json!({
                                                            "duration_seconds": duration.num_seconds(),
                                                            "until": until,
                                                            "reason": reason,
                                                            "status": "requested"
                                                        });
                                                        if record_moderation_event(
                                                            db, message.chat.id, telegram_id, target_id,
                                                            "mute_requested", details,
                                                        ).await.is_err() {
                                                            let _ = sqlx::query("DELETE FROM bot_chat_mutes WHERE chat_id = $1 AND target_telegram_id = $2")
                                                                .bind(message.chat.id.0).bind(target_id).execute(db).await;
                                                            "Не удалось записать действие в журнал; ограничение не применено.".into()
                                                        } else {
                                                            let applied = bot
                                                                .restrict_chat_member(
                                                                    message.chat.id,
                                                                    UserId(target_id as u64),
                                                                    ChatPermissions::empty(),
                                                                )
                                                                .until_date(until)
                                                                .await;
                                                            if applied.is_ok() {
                                                                let _ = record_moderation_event(
                                                                    db, message.chat.id, telegram_id, target_id,
                                                                    "mute_applied",
                                                                    serde_json::json!({"until": until}),
                                                                ).await;
                                                                format!(
                                                                    "🔇 Участник {} ограничен до {}. Для досрочного снятия: ответь на его сообщение командой /unmute.",
                                                                    target_id, moscow_time(until, "%d.%m %H:%M")
                                                                )
                                                            } else {
                                                                let _ = sqlx::query("DELETE FROM bot_chat_mutes WHERE chat_id = $1 AND target_telegram_id = $2")
                                                                    .bind(message.chat.id.0).bind(target_id).execute(db).await;
                                                                let _ = record_moderation_event(
                                                                    db, message.chat.id, telegram_id, target_id,
                                                                    "mute_failed", serde_json::json!({"reason": "Telegram API rejected the restriction"}),
                                                                ).await;
                                                                "Telegram не применил ограничение. Проверь, что бот — администратор с правом ограничивать участников.".into()
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        "/unmute" => {
            if !is_group_chat {
                "Снять мут можно только в групповом чате.".into()
            } else if !message.chat.is_supergroup() {
                "Для снятия ограничения нужна супергруппа Telegram.".into()
            } else if !crate::services::chat_roles::can_manage_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Снимать ограничения могут модераторы чата.".into()
            } else {
                match parse_target_and_tail(&message, argument) {
                    Err(error) => error,
                    Ok((_, ref tail)) if !tail.is_empty() => {
                        "Ответь на сообщение участника командой /unmute или укажи /unmute TELEGRAM_ID.".into()
                    }
                    Ok((target_id, _)) => {
                        let mute = sqlx::query_as::<_, (serde_json::Value, i64)>(
                            "SELECT original_permissions, actor_telegram_id FROM bot_chat_mutes WHERE chat_id = $1 AND target_telegram_id = $2",
                        )
                        .bind(message.chat.id.0)
                        .bind(target_id)
                        .fetch_optional(db)
                        .await;
                        match mute {
                            Ok(Some((permissions_json, _))) => {
                                let permissions = serde_json::from_value::<ChatPermissions>(permissions_json);
                                match permissions {
                                    Err(_) => "Не удалось восстановить сохранённые права чата.".into(),
                                    Ok(permissions) => {
                                        let member = bot.get_chat_member(message.chat.id, UserId(target_id as u64)).await;
                                        match member {
                                            Ok(member) if member.kind.is_owner() || member.kind.is_administrator() => {
                                                "Нельзя менять права владельца или администратора Telegram.".into()
                                            }
                                            Err(_) => "Не удалось проверить участника в Telegram.".into(),
                                            Ok(_) => {
                                                let requested = record_moderation_event(
                                                    db, message.chat.id, telegram_id, target_id,
                                                    "unmute_requested", serde_json::json!({}),
                                                ).await;
                                                if requested.is_err() {
                                                    "Не удалось записать действие в журнал; права не изменены.".into()
                                                } else if bot.restrict_chat_member(
                                                    message.chat.id, UserId(target_id as u64), permissions,
                                                ).await.is_err() {
                                                    let _ = record_moderation_event(
                                                        db, message.chat.id, telegram_id, target_id,
                                                        "unmute_failed", serde_json::json!({"reason": "Telegram API rejected the change"}),
                                                    ).await;
                                                    "Telegram не снял ограничение. Проверь права бота в чате.".into()
                                                } else {
                                                    let mut tx = match db.begin().await {
                                                        Ok(tx) => tx,
                                                        Err(_) => return reply(
                                                            &bot,
                                                            db,
                                                            &message,
                                                            "Права восстановлены, но не удалось завершить журналирование.".into(),
                                                            &state.config.webapp_url,
                                                        ).await,
                                                    };
                                                    let removed = sqlx::query("DELETE FROM bot_chat_mutes WHERE chat_id = $1 AND target_telegram_id = $2")
                                                        .bind(message.chat.id.0).bind(target_id).execute(&mut *tx).await;
                                                    let logged = sqlx::query(
                                                        "INSERT INTO bot_chat_audit_logs (chat_id, actor_telegram_id, target_telegram_id, action, details) VALUES ($1, $2, $3, 'unmute_applied', '{}'::jsonb)",
                                                    ).bind(message.chat.id.0).bind(telegram_id).bind(target_id).execute(&mut *tx).await;
                                                    if removed.is_ok() && logged.is_ok() && tx.commit().await.is_ok() {
                                                        format!("🔊 Ограничение участника {} снято. Восстановлены прежние права чата.", target_id)
                                                    } else {
                                                        "Права восстановлены, но не удалось завершить запись в журнал.".into()
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(None) => "Для этого участника нет активного ограничения SPBFTUIT.".into(),
                            Err(_) => "Не удалось прочитать данные ограничения.".into(),
                        }
                    }
                }
            }
        }

        "/ban" => {
            if !is_group_chat {
                "Блокировка работает только в групповом чате.".into()
            } else if !crate::services::chat_roles::can_manage_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Блокировать участников могут модераторы чата.".into()
            } else {
                match parse_target_and_tail(&message, argument) {
                    Err(error) => error,
                    Ok((target_id, _)) if target_id == telegram_id => {
                        "Нельзя заблокировать самого себя.".into()
                    }
                    Ok((_, ref reason)) if reason.chars().count() > 200 => {
                        "Причина должна быть не длиннее 200 символов.".into()
                    }
                    Ok((target_id, reason)) => {
                        match moderation_target_status(&bot, db, message.chat.id, target_id).await {
                            Err(error) => error,
                            Ok(_) => {
                                let reason = reason.trim();
                                if record_moderation_event(
                                    db,
                                    message.chat.id,
                                    telegram_id,
                                    target_id,
                                    "ban_requested",
                                    serde_json::json!({"reason": reason}),
                                )
                                .await
                                .is_err()
                                {
                                    "Не удалось записать действие в журнал; блокировка не применена.".into()
                                } else if bot
                                    .ban_chat_member(message.chat.id, UserId(target_id as u64))
                                    .revoke_messages(false)
                                    .await
                                    .is_ok()
                                {
                                    let _ = record_moderation_event(
                                        db,
                                        message.chat.id,
                                        telegram_id,
                                        target_id,
                                        "ban_applied",
                                        serde_json::json!({"reason": reason}),
                                    )
                                    .await;
                                    format!(
                                        "⛔ Участник {} заблокирован. Для разблокировки: /unban {}",
                                        target_id, target_id
                                    )
                                } else {
                                    let _ = record_moderation_event(
                                        db, message.chat.id, telegram_id, target_id,
                                        "ban_failed", serde_json::json!({"reason": "Telegram API rejected the ban"}),
                                    ).await;
                                    "Telegram не применил блокировку. Проверь, что бот — администратор с правом блокировать участников.".into()
                                }
                            }
                        }
                    }
                }
            }
        }

        "/unban" => {
            if !is_group_chat {
                "Разблокировка работает только в групповом чате.".into()
            } else if !crate::services::chat_roles::can_manage_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Разблокировать участников могут модераторы чата.".into()
            } else {
                match parse_target_and_tail(&message, argument) {
                    Err(error) => error,
                    Ok((_, ref tail)) if !tail.is_empty() => {
                        "Используй /unban TELEGRAM_ID или ответь на сообщение участника.".into()
                    }
                    Ok((target_id, _)) => {
                        if record_moderation_event(
                            db,
                            message.chat.id,
                            telegram_id,
                            target_id,
                            "unban_requested",
                            serde_json::json!({}),
                        )
                        .await
                        .is_err()
                        {
                            "Не удалось записать действие в журнал; разблокировка не выполнена."
                                .into()
                        } else if bot
                            .unban_chat_member(message.chat.id, UserId(target_id as u64))
                            .only_if_banned(true)
                            .await
                            .is_ok()
                        {
                            let _ = record_moderation_event(
                                db,
                                message.chat.id,
                                telegram_id,
                                target_id,
                                "unban_applied",
                                serde_json::json!({}),
                            )
                            .await;
                            format!("✅ Участник {} разблокирован и сможет вернуться по ссылке-приглашению.", target_id)
                        } else {
                            let _ = record_moderation_event(
                                db,
                                message.chat.id,
                                telegram_id,
                                target_id,
                                "unban_failed",
                                serde_json::json!({"reason": "Telegram API rejected the unban"}),
                            )
                            .await;
                            "Telegram не выполнил разблокировку. Проверь права бота в чате.".into()
                        }
                    }
                }
            }
        }

        "/chat_topic" => {
            if !is_group_chat {
                "Выбор темы доступен только в групповом чате.".into()
            } else if !matches!(
                crate::services::chat_roles::sync_member(&bot, db, message.chat.id, telegram_id, None)
                    .await.ok().flatten().as_deref(),
                Some("owner" | "admin")
            ) {
                "⛔ Выбирать тему публикаций может только создатель или администратор Telegram.".into()
            } else if !message.chat.is_supergroup() {
                "Темы доступны только в супергруппах Telegram.".into()
            } else if let Some(thread_id) = message.thread_id {
                match sqlx::query("UPDATE bot_chat_settings SET message_thread_id = $2, updated_at = now() WHERE chat_id = $1")
                    .bind(message.chat.id.0)
                    .bind(i64::from(thread_id.0.0))
                    .execute(db).await {
                    Ok(result) if result.rows_affected() == 1 => format!(
                        "✅ Тема сохранена (ID {}). Ежедневное расписание и уведомления будут отправляться сюда.",
                        thread_id.0.0
                    ),
                    Ok(_) => "Сначала привяжи учебную группу командой /setgroup название.".into(),
                    Err(error) => {
                        tracing::error!("Could not save Telegram topic: {}", error);
                        "Не удалось сохранить тему публикаций.".into()
                    }
                }
            } else {
                "Отправь /chat_topic прямо внутри нужной темы Telegram. В общем чате команда не выберет тему.".into()
            }
        }
        "/chat_topic_off" => {
            if !is_group_chat {
                "Настройка темы доступна только в групповом чате.".into()
            } else if !matches!(
                crate::services::chat_roles::sync_member(&bot, db, message.chat.id, telegram_id, None)
                    .await.ok().flatten().as_deref(),
                Some("owner" | "admin")
            ) {
                "⛔ Изменять тему публикаций может только создатель или администратор Telegram.".into()
            } else {
                match sqlx::query("UPDATE bot_chat_settings SET message_thread_id = NULL, updated_at = now() WHERE chat_id = $1")
                    .bind(message.chat.id.0)
                    .execute(db).await {
                    Ok(result) if result.rows_affected() == 1 => "✅ Публикации возвращены в общий чат.".into(),
                    Ok(_) => "Сначала привяжи учебную группу командой /setgroup название.".into(),
                    Err(error) => {
                        tracing::error!("Could not clear Telegram topic: {}", error);
                        "Не удалось изменить тему публикаций.".into()
                    }
                }
            }
        }
        "/daily_on" | "/daily_off" | "/chat_notify_on" | "/chat_notify_off" => {
            if !is_group_chat {
                "Эти настройки доступны только в групповом чате.".into()
            } else if !matches!(
                crate::services::chat_roles::sync_member(
                    &bot, db, message.chat.id, telegram_id, None
                ).await.ok().flatten().as_deref(),
                Some("owner" | "admin")
            ) {
                "⛔ Настраивать расписание и уведомления может только создатель или администратор Telegram.".into()
            } else {
                let (column, enabled, label) = match command {
                    "/daily_on" => ("daily_schedule_enabled", true, "ежедневная публикация расписания"),
                    "/daily_off" => ("daily_schedule_enabled", false, "ежедневная публикация расписания"),
                    "/chat_notify_on" => ("lesson_notifications_enabled", true, "уведомления о начале пар"),
                    _ => ("lesson_notifications_enabled", false, "уведомления о начале пар"),
                };
                let sql = format!(
                    "UPDATE bot_chat_settings SET {} = $2, updated_at = now() WHERE chat_id = $1",
                    column
                );
                match sqlx::query(&sql)
                    .bind(message.chat.id.0)
                    .bind(enabled)
                    .execute(db)
                    .await
                {
                    Ok(result) if result.rows_affected() == 1 => format!(
                        "{} {}.\\nТекущие параметры: /chat_settings",
                        if enabled { "✅ Включена" } else { "🔕 Выключена" },
                        label
                    ),
                    Ok(_) => "Сначала привяжи учебную группу командой /setgroup название.".into(),
                    Err(error) => {
                        tracing::error!("Could not update group schedule automation: {}", error);
                        "Не удалось изменить настройки. Проверь, применена ли миграция.".into()
                    }
                }
            }
        }
        "/daily_time" => {
            if !is_group_chat {
                "Время публикации настраивается только в групповом чате.".into()
            } else if !matches!(
                crate::services::chat_roles::sync_member(
                    &bot, db, message.chat.id, telegram_id, None
                ).await.ok().flatten().as_deref(),
                Some("owner" | "admin")
            ) {
                "⛔ Менять время публикации может только создатель или администратор Telegram.".into()
            } else {
                match NaiveTime::parse_from_str(argument, "%H:%M") {
                    Ok(time) => match sqlx::query(
                        "UPDATE bot_chat_settings SET daily_schedule_time = $2, updated_at = now() WHERE chat_id = $1"
                    )
                    .bind(message.chat.id.0)
                    .bind(time)
                    .execute(db)
                    .await
                    {
                        Ok(result) if result.rows_affected() == 1 => format!(
                            "🕒 Время ежедневной публикации установлено: {} (московское время).",
                            time.format("%H:%M")
                        ),
                        Ok(_) => "Сначала привяжи группу командой /setgroup название.".into(),
                        Err(error) => {
                            tracing::error!("Could not update daily schedule time: {}", error);
                            "Не удалось изменить время публикации.".into()
                        }
                    },
                    Err(_) => "Укажи время в формате ЧЧ:ММ, например /daily_time 07:30.".into(),
                }
            }
        }
        "/chat_settings" => {
            if !is_group_chat {
                "Команда работает только в групповом чате.".into()
            } else {
                match sqlx::query_as::<_, (String, bool, String, bool, bool, Option<i64>)>(
                    r#"
                    SELECT g.name, c.daily_schedule_enabled,
                           to_char(c.daily_schedule_time, 'HH24:MI'),
                           c.lesson_notifications_enabled, c.is_active, c.message_thread_id
                    FROM bot_chat_settings c
                    JOIN groups g ON g.id = c.group_id
                    WHERE c.chat_id = $1
                    "#
                )
                .bind(message.chat.id.0)
                .fetch_optional(db)
                .await
                {
                    Ok(Some((group, daily, time, notifications, active, thread_id))) => format!(
                        "⚙️ Настройки учебного чата\\nГруппа: {}\\nАвтопубликация: {} (ежедневно в {})\\nУведомления о парах: {}\\nТема публикаций: {}\\nИнтеграция: {}\\n\\nКоманды администратора:\\n/chat_topic — выбрать текущую тему\\n/chat_topic_off — публиковать в общий чат\\n/daily_on — включить расписание каждый день\\n/daily_off — выключить\\n/daily_time ЧЧ:ММ — время публикации (МСК)\\n/chat_notify_on — уведомлять за 10 минут до пары\\n/chat_notify_off — выключить уведомления",
                        group,
                        if daily { "включена" } else { "выключена" },
                        time,
                        if notifications { "включены" } else { "выключены" },
                        thread_id.map(|id| format!("тема #{}", id)).unwrap_or_else(|| "общий чат".into()),
                        if active { "активна" } else { "приостановлена" }
                    ),
                    Ok(None) => "Сначала привяжи учебную группу командой /setgroup название.".into(),
                    Err(error) => {
                        tracing::error!("Could not read group schedule settings: {}", error);
                        "Не удалось прочитать настройки учебного чата.".into()
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
                } else if telegram_id != state.config.admin_telegram_id {
                    "Добавлять задания может только администратор проекта. Просматривать задания можно всем участникам группы.".into()
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
                      AND chat_id = $2
                      AND id IN (
                        SELECT id
                        FROM bot_messages
                        WHERE telegram_id = $1
                          AND chat_id = $2
                        ORDER BY id DESC
                        LIMIT 30
                      )
                    RETURNING message_id
                    "#,
                )
                .bind(telegram_id)
                .bind(message.chat.id.0)
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
                    Ok(report) if report.groups_failed == 0 => format!(
                        "✅ Синхронизация завершена.\nГрупп обработано: {}/{}\nЗанятий импортировано: {}",
                        report.groups_successful,
                        report.groups_total,
                        report.lessons_imported
                    ),
                    Ok(report) => format!(
                        "⚠️ Синхронизация завершена частично.\nУспешно: {}/{} групп\nОшибок: {}\nЗанятий импортировано: {}",
                        report.groups_successful,
                        report.groups_total,
                        report.groups_failed,
                        report.lessons_imported
                    ),
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

    reply(&bot, db, &message, answer, &state.config.webapp_url).await?;
    Ok(())
}

async fn register_bot_commands(bot: &Bot) {
    let private_commands = vec![
        BotCommand::new("start", "Открыть меню"),
        BotCommand::new("help", "Список команд"),
        BotCommand::new("today", "Расписание на сегодня"),
        BotCommand::new("tomorrow", "Расписание на завтра"),
        BotCommand::new("week", "Расписание на 7 дней"),
        BotCommand::new("next", "Следующая пара"),
        BotCommand::new("group", "Выбрать личную учебную группу"),
        BotCommand::new("homework", "Домашние задания"),
        BotCommand::new("remind_on", "Включить напоминания"),
        BotCommand::new("remind_off", "Выключить напоминания"),
        BotCommand::new("reminders", "Статус напоминаний"),
        BotCommand::new("settings", "Мои настройки"),
        BotCommand::new("clear", "Удалить последние сообщения бота"),
        BotCommand::new("sync", "Обновить расписание из ЭИОС (администратор)"),
    ];

    if let Err(error) = bot
        .set_my_commands(private_commands)
        .scope(BotCommandScope::AllPrivateChats)
        .language_code("ru")
        .await
    {
        tracing::warn!("Could not register private Telegram commands: {}", error);
    }

    let group_commands = vec![
        BotCommand::new("start", "Открыть меню"),
        BotCommand::new("help", "Список команд"),
        BotCommand::new("today", "Расписание на сегодня"),
        BotCommand::new("tomorrow", "Расписание на завтра"),
        BotCommand::new("week", "Расписание на 7 дней"),
        BotCommand::new("next", "Следующая пара"),
        BotCommand::new("homework", "Домашние задания"),
        BotCommand::new("settings", "Настройки учебного чата"),
        BotCommand::new("chat_settings", "Настройки расписания учебного чата"),
        BotCommand::new("daily_on", "Ежедневно публиковать расписание в чате"),
        BotCommand::new("daily_off", "Отключить ежедневное расписание в чате"),
        BotCommand::new("daily_time", "Установить время публикации ЧЧ:ММ (МСК)"),
        BotCommand::new("chat_notify_on", "Уведомлять о парах в чате"),
        BotCommand::new("chat_notify_off", "Отключить уведомления о парах в чате"),
        BotCommand::new("warn", "Выдать предупреждение участнику"),
        BotCommand::new("warnings", "Посмотреть предупреждения участника"),
        BotCommand::new("mute", "Ограничить участника на время"),
        BotCommand::new("unmute", "Снять временное ограничение"),
        BotCommand::new("ban", "Заблокировать участника"),
        BotCommand::new("unban", "Разблокировать участника"),
    ];

    if let Err(error) = bot
        .set_my_commands(group_commands.clone())
        .scope(BotCommandScope::AllGroupChats)
        .language_code("ru")
        .await
    {
        tracing::warn!("Could not register group Telegram commands: {}", error);
    }

    let mut admin_commands = group_commands;
    admin_commands.push(BotCommand::new("chat_topic", "Выбрать текущую тему для публикаций"));
    admin_commands.push(BotCommand::new("chat_topic_off", "Публиковать в общий чат"));
    admin_commands.push(BotCommand::new(
        "setgroup",
        "Привязать учебную группу к чату",
    ));
    admin_commands.push(BotCommand::new(
        "role",
        "Назначить роль по @username или ответом",
    ));

    if let Err(error) = bot
        .set_my_commands(admin_commands)
        .scope(BotCommandScope::AllChatAdministrators)
        .language_code("ru")
        .await
    {
        tracing::warn!(
            "Could not register administrator Telegram commands: {}",
            error
        );
    }
}

pub async fn run(state: Arc<AppState>) {
    let Some(token) = state.config.teloxide_token.clone() else {
        tracing::warn!("TELOXIDE_TOKEN is not configured; bot is disabled");
        return;
    };

    let bot = Bot::new(token);
    register_bot_commands(&bot).await;

    let reminder_bot = bot.clone();
    let reminder_state = state.clone();

    tokio::spawn(async move {
        reminder_loop(reminder_bot, reminder_state).await;
    });

    let message_state = state.clone();
    let inline_state = state.clone();

    let handler = dptree::entry()
        .branch(
            Update::filter_message().endpoint(move |bot: Bot, message: Message| {
                let state = message_state.clone();
                async move { handle(bot, state, message).await }
            }),
        )
        .branch(
            Update::filter_inline_query().endpoint(move |bot: Bot, query: InlineQuery| {
                let state = inline_state.clone();
                async move { handle_inline_query(bot, state, query).await }
            }),
        );

    Dispatcher::builder(bot, handler).build().dispatch().await;
}


/// Delivers opt-in daily schedule posts and 10-minute lesson reminders to configured group chats.
/// Delivery rows are reserved before sending to prevent duplicate messages across worker ticks.
async fn group_chat_schedule_loop_tick(bot: &Bot, state: &Arc<AppState>) {
    let daily_chats = sqlx::query_as::<_, (i64, uuid::Uuid, NaiveDate, Option<i64>)>(
        r#"
        SELECT c.chat_id, c.group_id, (now() AT TIME ZONE 'Europe/Moscow')::date, c.message_thread_id
        FROM bot_chat_settings c
        WHERE c.is_active = TRUE
          AND c.daily_schedule_enabled = TRUE
          AND c.daily_schedule_time <= (now() AT TIME ZONE 'Europe/Moscow')::time
          AND NOT EXISTS (
              SELECT 1 FROM bot_chat_daily_posts p
              WHERE p.chat_id = c.chat_id
                AND p.local_date = (now() AT TIME ZONE 'Europe/Moscow')::date
                AND p.delivered_at IS NOT NULL
          )
        "#
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|error| {
        tracing::error!("Could not load daily group schedule posts: {}", error);
        Vec::new()
    });

    for (chat_id, group_id, local_date, thread_id) in daily_chats {
        let reserved = sqlx::query(
            r#"
            INSERT INTO bot_chat_daily_posts (chat_id, local_date, sent_at)
            VALUES ($1, $2, now())
            ON CONFLICT (chat_id, local_date) DO UPDATE
                SET sent_at = now()
                WHERE bot_chat_daily_posts.delivered_at IS NULL
                  AND bot_chat_daily_posts.sent_at < now() - interval '2 minutes'
            "#
        )
        .bind(chat_id)
        .bind(local_date)
        .execute(&state.db)
        .await;

        if !matches!(reserved, Ok(ref result) if result.rows_affected() == 1) {
            continue;
        }

        let text = schedule_text(&state.db, group_id, "/today").await;
        let message = format!("📢 Ежедневное расписание учебной группы\\n\\n{}", text);
        let request = bot.send_message(ChatId(chat_id), message);
        let request = match thread_id.and_then(|id| i32::try_from(id).ok()) {
            Some(id) => request.message_thread_id(ThreadId(MessageId(id))),
            None => request,
        };
        match request.await {
            Ok(_) => {
                if let Err(error) = sqlx::query(
                    "UPDATE bot_chat_daily_posts SET delivered_at = now() WHERE chat_id = $1 AND local_date = $2"
                ).bind(chat_id).bind(local_date).execute(&state.db).await {
                    tracing::error!("Could not mark daily schedule as delivered to chat {}: {}", chat_id, error);
                }
            }
            Err(error) => {
            tracing::warn!("Could not post daily schedule to chat {}: {}", chat_id, error);
            let _ = sqlx::query(
                "DELETE FROM bot_chat_daily_posts WHERE chat_id = $1 AND local_date = $2"
            )
            .bind(chat_id)
            .bind(local_date)
            .execute(&state.db)
            .await;
            }
        }
    }

    let lessons = sqlx::query_as::<_, (i64, String, String, DateTime<Utc>, Option<i64>)>(
        r#"
        SELECT c.chat_id, l.id::text, l.subject, l.starts_at, c.message_thread_id
        FROM bot_chat_settings c
        JOIN lessons l ON l.group_id = c.group_id
        WHERE c.is_active = TRUE
          AND c.lesson_notifications_enabled = TRUE
          AND l.starts_at BETWEEN now() + interval '9 minutes'
                              AND now() + interval '10 minutes'
          AND NOT EXISTS (
              SELECT 1
              FROM bot_chat_lesson_notifications n
              WHERE n.chat_id = c.chat_id AND n.lesson_id = l.id
          )
        ORDER BY l.starts_at
        "#
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|error| {
        tracing::error!("Could not load group lesson notifications: {}", error);
        Vec::new()
    });

    for (chat_id, lesson_id, subject, starts_at, thread_id) in lessons {
        let Ok(lesson_uuid) = uuid::Uuid::parse_str(&lesson_id) else { continue };
        let reserved = sqlx::query(
            "INSERT INTO bot_chat_lesson_notifications (chat_id, lesson_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"
        )
        .bind(chat_id)
        .bind(lesson_uuid)
        .execute(&state.db)
        .await;

        if !matches!(reserved, Ok(ref result) if result.rows_affected() == 1) {
            continue;
        }

        let text = format!(
            "🔔 Скоро начинается пара!\\n\\n📚 {}\\n🕒 Начало: {}\\n⏳ До начала примерно 10 минут",
            subject,
            moscow_time(starts_at, "%H:%M")
        );
        let request = bot.send_message(ChatId(chat_id), text);
        let request = match thread_id.and_then(|id| i32::try_from(id).ok()) {
            Some(id) => request.message_thread_id(ThreadId(MessageId(id))),
            None => request,
        };
        if let Err(error) = request.await {
            tracing::warn!("Could not send lesson notification to chat {}: {}", chat_id, error);
            let _ = sqlx::query(
                "DELETE FROM bot_chat_lesson_notifications WHERE chat_id = $1 AND lesson_id = $2"
            )
            .bind(chat_id)
            .bind(lesson_uuid)
            .execute(&state.db)
            .await;
        }
    }
}

async fn reminder_loop(bot: Bot, state: Arc<AppState>) {
    let mut tick = tokio::time::interval(Duration::from_secs(30));

    loop {
        tick.tick().await;

        let _ = sqlx::query("DELETE FROM bot_chat_mutes WHERE muted_until <= now()")
            .execute(&state.db)
            .await;

        group_chat_schedule_loop_tick(&bot, &state).await;

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

                match inserted {
                    Ok(result) if result.rows_affected() == 1 => {
                        match bot
                            .send_message(
                                ChatId(telegram_id),
                                format!(
                                    "⏰ Через 10 минут пара: {}\nНачало: {}",
                                    subject,
                                    moscow_time(starts_at, "%H:%M")
                                ),
                            )
                            .await
                        {
                            Ok(_) => {}
                            Err(error) => {
                                tracing::warn!(
                                    "Could not deliver lesson reminder to {}: {}",
                                    telegram_id,
                                    error
                                );
                                let _ = sqlx::query(
                                    "DELETE FROM bot_reminder_deliveries WHERE telegram_id = $1 AND lesson_id = $2"
                                ).bind(telegram_id).bind(id).execute(&state.db).await;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => tracing::error!(
                        "Could not reserve reminder delivery for {}: {}",
                        telegram_id,
                        error
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn inline_search_understands_russian_schedule_queries() {
        assert_eq!(inline_actions("следующая пара")[0].id, "inline_next");
        assert_eq!(inline_actions("ближайшая пара")[0].id, "inline_next");
        assert_eq!(inline_actions("7 дней")[0].id, "inline_week");
        assert_eq!(inline_actions("завтра")[0].id, "inline_tomorrow");
    }

    use super::*;

    #[test]
    fn mute_duration_has_safe_bounds_and_units() {
        assert_eq!(parse_mute_duration("1m"), Some(ChronoDuration::minutes(1)));
        assert_eq!(parse_mute_duration("12h"), Some(ChronoDuration::hours(12)));
        assert_eq!(parse_mute_duration("30d"), Some(ChronoDuration::days(30)));
        assert_eq!(parse_mute_duration("0m"), None);
        assert_eq!(parse_mute_duration("31d"), None);
        assert_eq!(parse_mute_duration("999999999999999999999d"), None);
        assert_eq!(parse_mute_duration("15м"), None);
    }
}
