use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc,
};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{sync::Arc, time::Duration};
use teloxide::{
    prelude::*,
    types::{
        BotCommand, BotCommandScope, ButtonRequest, CallbackQuery, ChatPermissions, ParseMode,
        InlineKeyboardButton, InlineKeyboardMarkup, InlineQuery, InlineQueryResult,
        InlineQueryResultArticle, InputFile, InputMessageContent, InputMessageContentText,
        KeyboardButton, KeyboardMarkup, Message, MessageId, Recipient, ReplyParameters, ThreadId, UserId, Update, UpdateKind,
        WebAppInfo,
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
            KeyboardButton::new("📚 ДЗ"),
            KeyboardButton::new("🗓 Расписание"),
            KeyboardButton::new("⏰ Дедлайны"),
        ],
        vec![
            KeyboardButton::new("🏫 Аудитории"),
            KeyboardButton::new("👨‍🏫 Преподаватель"),
        ],
        vec![
            KeyboardButton::new("⚙️ Настройки"),
            KeyboardButton::new("❓ Помощь"),
        ],
    ];

    if is_private {
        rows.push(vec![
            KeyboardButton::new("👤 Кабинет"),
            KeyboardButton::new("💚 Знакомства"),
        ]);
        if let Ok(url) = url::Url::parse(webapp_url) {
            if url.scheme() == "https" && url.host_str() != Some("example.com") {
                rows.push(vec![KeyboardButton::new("🌲 Открыть SPBFTUIT")
                    .request(ButtonRequest::WebApp(WebAppInfo { url }))]);
            }
        }
    }

    KeyboardMarkup::new(rows)
        .resize_keyboard()
        .persistent()
        .input_field_placeholder("Выбери раздел")
}

fn is_main_menu_button(text: &str) -> bool {
    matches!(
        text.trim(),
        "📚 ДЗ"
            | "🗓 Расписание"
            | "⏰ Дедлайны"
            | "🏫 Аудитории"
            | "👨‍🏫 Преподаватель"
            | "⚙️ Настройки"
            | "❓ Помощь"
            | "👤 Кабинет"
            | "💚 Знакомства"
            | "📅 Сегодня"
            | "📅 Завтра"
            | "🗓 Неделя"
            | "⏭ Следующая пара"
            | "📚 Задания"
            | "🔔 Напоминания"
            | "🏠 Меню"
    )
}

/// Reply-keyboard labels are actions, not command arguments.
/// Without this guard, pressing "📚 ДЗ" was parsed as "/homework ДЗ"
/// and the bot attempted to add a homework item named "ДЗ".
fn message_argument(text: &str) -> &str {
    if is_main_menu_button(text) {
        ""
    } else {
        text.splitn(2, char::is_whitespace)
            .nth(1)
            .unwrap_or("")
            .trim()
    }
}

fn ui_button(label: &str, action: &str) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(label, action)
}

fn ui_keyboard(
    screen: &str,
    reminders_enabled: bool,
    reminder_minutes: i64,
) -> InlineKeyboardMarkup {
    let rows = match screen {
        "onboarding-1" => vec![vec![ui_button("Дальше →", "ui:onboarding:2")]],
        "onboarding-2" => vec![
            vec![ui_button("Дальше →", "ui:onboarding:3")],
            vec![ui_button("⬅️ Назад", "ui:onboarding:1")],
        ],
        "onboarding-3" => vec![
            vec![ui_button("🏠 Открыть меню", "ui:onboarding:done")],
            vec![ui_button("⬅️ Назад", "ui:onboarding:2")],
        ],
        "schedule" => vec![
            vec![
                ui_button("Сегодня", "ui:schedule:today"),
                ui_button("Завтра", "ui:schedule:tomorrow"),
            ],
            vec![
                ui_button("7 дней", "ui:schedule:week"),
                ui_button("Следующая пара", "ui:schedule:next"),
            ],
            vec![ui_button("🏠 В меню", "ui:main")],
        ],
        "settings" => vec![
            vec![ui_button(
                if reminders_enabled {
                    "🔔 Напоминания: ВКЛ ✅"
                } else {
                    "🔕 Напоминания: ВЫКЛ ❌"
                },
                "ui:reminders:toggle",
            )],
            vec![
                ui_button(
                    if reminder_minutes == 10 {
                        "✅ 10 мин"
                    } else {
                        "10 мин"
                    },
                    "ui:reminder:10",
                ),
                ui_button(
                    if reminder_minutes == 60 {
                        "✅ 1 час"
                    } else {
                        "1 час"
                    },
                    "ui:reminder:60",
                ),
                ui_button(
                    if reminder_minutes == 180 {
                        "✅ 3 часа"
                    } else {
                        "3 часа"
                    },
                    "ui:reminder:180",
                ),
            ],
            vec![ui_button(
                if reminder_minutes == 1440 {
                    "✅ 1 день"
                } else {
                    "1 день"
                },
                "ui:reminder:1440",
            )],
            vec![ui_button("📨 Ежедневный дайджест · 07:00 МСК", "ui:digest:toggle")],
            vec![ui_button("🔔 Изменения расписания", "ui:changes:toggle")],
            vec![ui_button("🏠 В меню", "ui:main")],
        ],
        "homework" => vec![vec![
            ui_button("⬅️ Назад", "ui:main"),
            ui_button("🏠 В меню", "ui:main"),
        ]],
        "deadlines" => vec![vec![
            ui_button("⬅️ Назад", "ui:main"),
            ui_button("🏠 В меню", "ui:main"),
        ]],
        "help" => vec![
            vec![ui_button("📋 Список команд", "ui:commands")],
            vec![ui_button("🏠 В меню", "ui:main")],
        ],
        _ => vec![
            vec![
                ui_button("🗓 Расписание", "ui:schedule"),
                ui_button("✨ Что дальше?", "ui:home"),
            ],
            vec![
                ui_button("📚 ДЗ", "ui:homework"),
                ui_button("⏰ Дедлайны", "ui:deadlines"),
            ],
            vec![
                ui_button("👨‍🏫 Преподаватель", "ui:teacher"),
                ui_button("🏫 Аудитории", "ui:rooms"),
            ],
            vec![
                ui_button("⚙️ Настройки", "ui:settings"),
                ui_button("❓ Помощь", "ui:help"),
            ],
            vec![
                ui_button("👤 Кабинет", "pc:open"),
                ui_button("💚 Знакомства", "dating:open"),
            ],
        ],
    };
    InlineKeyboardMarkup::new(rows)
}

fn chat_settings_keyboard(
    chat_id: ChatId,
    group_name: &str,
    daily_enabled: bool,
    lesson_enabled: bool,
    digest_enabled: bool,
    assistant_enabled: bool,
    context_enabled: bool,
    homework_autocollect_enabled: bool,
    homework_approver_role: &str,
) -> InlineKeyboardMarkup {
    let _ = (chat_id, group_name);
    InlineKeyboardMarkup::new(vec![
        vec![ui_button("🏫 Изменить учебную группу", "chatgroup:open")],
        vec![ui_button(
            if daily_enabled { "✅ Ежедневное расписание: ВКЛ" } else { "Ежедневное расписание: ВЫКЛ" },
            "chatsettings:daily",
        )],
        vec![ui_button(
            if lesson_enabled { "✅ Уведомления о парах: ВКЛ" } else { "Уведомления о парах: ВЫКЛ" },
            "chatsettings:lessons",
        )],
        vec![ui_button(
            if digest_enabled { "✅ Дайджест группы: ВКЛ · 07:00 МСК" } else { "Дайджест группы: ВЫКЛ · 07:00 МСК" },
            "chatsettings:digest",
        )],
        vec![ui_button(
            if assistant_enabled { "✅ Помощник: ВКЛ" } else { "Помощник: ВЫКЛ" },
            "chatsettings:assistant",
        )],
        vec![ui_button(
            if context_enabled { "✅ Контекст ответов: ВКЛ" } else { "Контекст ответов: ВЫКЛ" },
            "chatsettings:context",
        )],
        vec![ui_button(
            if homework_autocollect_enabled { "✅ Автосбор ДЗ: ВКЛ" } else { "Автосбор ДЗ: ВЫКЛ" },
            "chatsettings:autodz",
        )],
        vec![ui_button(
            &format!("Подтверждают ДЗ: {}", match homework_approver_role {
                "class_leader" => "староста",
                "moderator" => "модераторы",
                _ => "администраторы",
            }),
            "chatsettings:approver",
        )],
        vec![ui_button("🏠 Меню", "ui:main")],
    ])
}

fn commands_text(in_group: bool) -> String {
    let mut text = String::from(
        "📋 КОМАНДЫ БОТА\n\nОбычные разделы удобнее открывать кнопками. Аргументы нужны только для поиска и управления данными.\n\n🎓 Основное\n/start — начать или вернуться\n/menu — открыть меню\n/help — подсказки\n/commands — этот список\n/settings — настройки\n/today, /tomorrow, /week, /next — расписание\n/homework, /deadlines, /home — задания и что дальше\n/teacher Фамилия — поиск преподавателя\n/free_rooms [ЧЧ:ММ] — свободные аудитории\n/group название — выбрать личную группу\n",
    );
    if in_group {
        text.push_str(
            "\n📚 Учебная группа\n/dz — список ДЗ\n/add_dz Предмет | описание | срок — добавить\n/edit_dz ID | предмет | описание | срок — изменить\n/delete_dz ID — удалить с подтверждением\n/attach_dz ID — подпись к фото/файлу для вложения\n/done ID, /my_dz — отметить и посмотреть своё\n/exams, /events — экзамены и мероприятия\n/resources — материалы\n/lost, /found, /lost_found — потерянные вещи\n/report — жалоба ответом на сообщение\n/poll Вопрос | вариант 1 | вариант 2 — опрос\n\n⭐ Старосте и администраторам\n/autodz_on, /autodz_off — автосбор ДЗ\n/autodz_approver class_leader|moderator|admin — кто подтверждает\n/subject_alias Предмет | сокращение — словарь предметов\n/add_resource Название | https://ссылка — материалы\n/setgroup название, /chat_settings — настройка чата\n\n🛡 Модераторам и администраторам\n/roles, /role — роли\nОтветом: /warn причина, /mute 30m причина, /unmute, /ban\n/warnings ID, /unban ID — журнал и разблокировка\n/report_queue, /report_resolve ID — жалобы\n\nПолный формат команды покажет /help. Команды управления доступны только ролям с нужными правами.",
        );
    }
    text
}

async fn start_onboarding(
    bot: &Bot,
    state: &Arc<AppState>,
    message: &Message,
) -> ResponseResult<()> {
    let telegram_id = match telegram_user_id(message) {
        Some(id) => id,
        None => return Ok(()),
    };
    let complete: bool = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT settings FROM users WHERE telegram_id=$1",
    )
    .bind(telegram_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or_default()["onboarding_completed"]
        == true;
    if complete {
        reply(
            bot,
            &state.db,
            message,
            "С возвращением! Выбери раздел в меню ниже или нажми /menu.".into(),
            &state.config.webapp_url,
        )
        .await?;
        return Ok(());
    }
    bot.send_message(
        message.chat.id,
        "Используй постоянные кнопки внизу для основных разделов.",
    )
    .reply_markup(main_keyboard(&state.config.webapp_url, true))
    .await?;
    bot.send_message(message.chat.id, "Привет! Я помогу с расписанием, заданиями и жизнью учебной группы.\n\nШаг 1 из 3 · выбери учебную группу командой /group или кнопкой Mini App ниже.")
        .reply_markup(ui_keyboard("onboarding-1", false, 10)).await?;
    Ok(())
}

async fn handle_group_setup_callback(
    bot: &Bot,
    state: &AppState,
    query: &CallbackQuery,
) -> ResponseResult<bool> {
    let Some(data) = query.data.as_deref() else { return Ok(false); };
    if !data.starts_with("chatgroup:") { return Ok(false); }
    let Some(message) = query.regular_message() else {
        bot.answer_callback_query(query.id.clone())
            .text("Открой настройку группы непосредственно в Telegram-чате.")
            .show_alert(true).await?;
        return Ok(true);
    };
    let chat_id = message.chat.id;
    let actor = i64::try_from(query.from.id.0).unwrap_or_default();
    if chat_id.0 >= 0 {
        bot.answer_callback_query(query.id.clone())
            .text("Эта настройка доступна только в групповом чате.")
            .show_alert(true).await?;
        return Ok(true);
    }
    let role = crate::services::chat_roles::sync_member(
        bot, &state.db, chat_id, actor, None
    ).await.ok().flatten();
    if !matches!(role.as_deref(), Some("owner" | "admin")) {
        bot.answer_callback_query(query.id.clone())
            .text("Менять учебную группу могут только администраторы чата.")
            .show_alert(true).await?;
        return Ok(true);
    }

    if data == "chatgroup:open" || data.starts_with("chatgroup:page:") {
        let page = data.strip_prefix("chatgroup:page:")
            .and_then(|v| v.parse::<i64>().ok()).unwrap_or(0).clamp(0, 1000);
        const PAGE_SIZE: i64 = 8;
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM groups WHERE is_active=TRUE")
            .fetch_one(&state.db).await.unwrap_or(0);
        let pages = ((total + PAGE_SIZE - 1) / PAGE_SIZE).max(1);
        let page = page.min(pages - 1);
        let rows = sqlx::query_as::<_, (uuid::Uuid, String)>(
            "SELECT id,name FROM groups WHERE is_active=TRUE ORDER BY name LIMIT $1 OFFSET $2"
        ).bind(PAGE_SIZE).bind(page * PAGE_SIZE).fetch_all(&state.db).await;
        let Ok(groups) = rows else {
            bot.answer_callback_query(query.id.clone())
                .text("Не удалось загрузить список групп.")
                .show_alert(true).await?;
            return Ok(true);
        };
        let mut keyboard = groups.into_iter().map(|(id,name)| vec![
            ui_button(&name, &format!("chatgroup:select:{id}"))
        ]).collect::<Vec<_>>();
        let mut page_row = Vec::new();
        if page > 0 { page_row.push(ui_button("◀️ Назад", &format!("chatgroup:page:{}", page-1))); }
        if page+1 < pages { page_row.push(ui_button("Далее ▶️", &format!("chatgroup:page:{}", page+1))); }
        if !page_row.is_empty() { keyboard.push(page_row); }
        keyboard.push(vec![ui_button("🏠 Меню", "ui:main")]);
        bot.answer_callback_query(query.id.clone()).await?;
        let text = format!("🏫 Выбери учебную группу для этого чата. Страница {} из {} · групп: {}",page+1,pages,total);
        let edit = bot.edit_message_text(chat_id, message.id, text)
            .reply_markup(InlineKeyboardMarkup::new(keyboard)).await;
        if let Err(error)=edit {
            if !error.to_string().contains("message is not modified") { return Err(error.into()); }
        }
        return Ok(true);
    }

    if let Some(id_text)=data.strip_prefix("chatgroup:select:") {
        let Ok(group_id)=uuid::Uuid::parse_str(id_text) else {
            bot.answer_callback_query(query.id.clone()).text("Некорректный ID группы.").show_alert(true).await?;
            return Ok(true);
        };
        let group = sqlx::query_as::<_,(String,)>(
            "SELECT name FROM groups WHERE id=$1 AND is_active=TRUE"
        ).bind(group_id).fetch_optional(&state.db).await;
        let Ok(Some((name,)))=group else {
            bot.answer_callback_query(query.id.clone()).text("Группа не найдена или неактивна.").show_alert(true).await?;
            return Ok(true);
        };
        let saved=sqlx::query(
            "INSERT INTO bot_chat_settings(chat_id,group_id,configured_by,updated_at) VALUES($1,$2,$3,now()) ON CONFLICT(chat_id) DO UPDATE SET group_id=EXCLUDED.group_id,configured_by=EXCLUDED.configured_by,updated_at=now()"
        ).bind(chat_id.0).bind(group_id).bind(actor).execute(&state.db).await;
        if let Err(error)=saved {
            tracing::error!("Could not save chat group selection: {}",error);
            bot.answer_callback_query(query.id.clone()).text("Не удалось сохранить группу в Neon.").show_alert(true).await?;
            return Ok(true);
        }
        bot.answer_callback_query(query.id.clone()).text("Группа чата сохранена.").await?;
        bot.edit_message_text(chat_id,message.id,format!("✅ Учебная группа этого чата: {name}"))
            .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                ui_button("🏫 Изменить группу","chatgroup:open"),
                ui_button("⚙️ Настройки чата","chatsettings:open"),
            ],vec![ui_button("🏠 Меню","ui:main")]]))
            .await?;
        return Ok(true);
    }

    bot.answer_callback_query(query.id.clone()).text("Кнопка устарела.").await?;
    Ok(true)
}

async fn handle_chat_settings_callback(
    bot: &Bot,
    state: &AppState,
    query: &CallbackQuery,
) -> ResponseResult<bool> {
    let Some(action) = query.data.as_deref() else { return Ok(false); };
    if action != "chatsettings:open" && !action.starts_with("chatsettings:") {
        return Ok(false);
    }
    let Some(message) = query.regular_message() else {
        bot.answer_callback_query(query.id.clone())
            .text("Открой настройки в сообщении группового чата.")
            .show_alert(true).await?;
        return Ok(true);
    };
    let chat_id = message.chat.id;
    let actor = i64::try_from(query.from.id.0).unwrap_or_default();
    if chat_id.0 >= 0 {
        bot.answer_callback_query(query.id.clone())
            .text("Настройки доступны только в групповом чате.")
            .show_alert(true).await?;
        return Ok(true);
    }
    let role = crate::services::chat_roles::sync_member(
        bot, &state.db, chat_id, actor, None
    ).await.ok().flatten();
    if !matches!(role.as_deref(), Some("owner" | "admin")) {
        bot.answer_callback_query(query.id.clone())
            .text("Настраивать учебный чат могут только администраторы Telegram.")
            .show_alert(true).await?;
        return Ok(true);
    }

    match action {
        "chatsettings:context" => {
            let enabled = sqlx::query_scalar::<_, bool>(
                "SELECT context_enabled FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE"
            ).bind(chat_id.0).fetch_optional(&state.db).await.ok().flatten().unwrap_or(false);
            bot.answer_callback_query(query.id.clone()).await?;
            if !enabled {
                let text = "🔐 СОГЛАСИЕ НА КОНТЕКСТНЫЕ ОТВЕТЫ\n\nЕсли включить эту функцию, бот будет сохранять только сообщения, которыми участники отвечают на сообщения самого бота в этом учебном чате.\n\n• Обычная переписка без ответа боту не сохраняется.\n• История привязана только к этому chat_id и не смешивается с другими чатами.\n• Контекст хранится до 7 дней, затем автоматически очищается.\n• Администратор может отключить функцию и удалить историю в любой момент.\n\nПодтверди согласие, чтобы включить сбор этого ограниченного контекста.";
                let keyboard = InlineKeyboardMarkup::new(vec![
                    vec![ui_button("✅ Подтверждаю и включаю", "chatsettings:context:confirm")],
                    vec![ui_button("Отмена", "chatsettings:open")],
                ]);
                bot.edit_message_text(chat_id, message.id, text).reply_markup(keyboard).await?;
            } else {
                let text = "Отключить контекстные ответы и удалить всю сохранённую историю этого чата?";
                let keyboard = InlineKeyboardMarkup::new(vec![
                    vec![ui_button("🗑 Отключить и удалить историю", "chatsettings:context:disable")],
                    vec![ui_button("⬅️ Назад", "chatsettings:open")],
                ]);
                bot.edit_message_text(chat_id, message.id, text).reply_markup(keyboard).await?;
            }
            return Ok(true);
        }
        "chatsettings:context:confirm" => {
            let result = sqlx::query(
                "UPDATE bot_chat_settings SET context_enabled=TRUE,context_retention_days=7,updated_at=now() WHERE chat_id=$1 AND is_active=TRUE"
            ).bind(chat_id.0).execute(&state.db).await;
            if !matches!(result, Ok(ref r) if r.rows_affected()==1) {
                bot.answer_callback_query(query.id.clone()).text("Не удалось включить контекст.").show_alert(true).await?;
                return Ok(true);
            }
            let _ = sqlx::query(
                "INSERT INTO bot_chat_assistant_settings_audit(chat_id,changed_by,setting,value) VALUES($1,$2,'context_enabled','true'::jsonb)"
            ).bind(chat_id.0).bind(actor).execute(&state.db).await;
        }
        "chatsettings:context:disable" => {
            let result = sqlx::query(
                "UPDATE bot_chat_settings SET context_enabled=FALSE,assistant_enabled=FALSE,updated_at=now() WHERE chat_id=$1"
            ).bind(chat_id.0).execute(&state.db).await;
            if !matches!(result, Ok(ref r) if r.rows_affected()==1) {
                bot.answer_callback_query(query.id.clone()).text("Не удалось отключить контекст.").show_alert(true).await?;
                return Ok(true);
            }
            if let Err(error) = sqlx::query("DELETE FROM bot_chat_context_messages WHERE chat_id=$1")
                .bind(chat_id.0).execute(&state.db).await {
                tracing::error!("Could not erase chat context after opt-out: {}", error);
            }
            let _ = sqlx::query(
                "INSERT INTO bot_chat_assistant_settings_audit(chat_id,changed_by,setting,value) VALUES($1,$2,'context_enabled','false'::jsonb)"
            ).bind(chat_id.0).bind(actor).execute(&state.db).await;
        }
        "chatsettings:assistant" => {
            let row = sqlx::query_as::<_, (bool, bool)>(
                "SELECT assistant_enabled,context_enabled FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE"
            ).bind(chat_id.0).fetch_optional(&state.db).await;
            let Ok(Some((assistant_enabled, context_enabled))) = row else {
                bot.answer_callback_query(query.id.clone()).text("Сначала настрой учебную группу.").show_alert(true).await?;
                return Ok(true);
            };
            if !assistant_enabled && !context_enabled {
                bot.answer_callback_query(query.id.clone())
                    .text("Сначала включи контекстные ответы и подтверди согласие.")
                    .show_alert(true).await?;
                return Ok(true);
            }
            let enabled = !assistant_enabled;
            let result = sqlx::query(
                "UPDATE bot_chat_settings SET assistant_enabled=$2,updated_at=now() WHERE chat_id=$1"
            ).bind(chat_id.0).bind(enabled).execute(&state.db).await;
            if let Err(error) = result {
                tracing::error!("Could not change group assistant setting: {}", error);
                bot.answer_callback_query(query.id.clone()).text("Не удалось сохранить настройку помощника.").show_alert(true).await?;
                return Ok(true);
            }
            let _ = sqlx::query(
                "INSERT INTO bot_chat_assistant_settings_audit(chat_id,changed_by,setting,value) VALUES($1,$2,'assistant_enabled',to_jsonb($3::boolean))"
            ).bind(chat_id.0).bind(actor).bind(enabled).execute(&state.db).await;
        }
        "chatsettings:autodz" => {
            let can_manage: bool = sqlx::query_scalar(
                r#"SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND owner_telegram_id=$2 AND is_active=TRUE)
                   OR EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND telegram_status IN ('creator','administrator'))
                   OR EXISTS(SELECT 1 FROM bot_chat_member_roles WHERE chat_id=$1 AND telegram_id=$2 AND role='class_leader')"#
            ).bind(chat_id.0).bind(actor).fetch_one(&state.db).await.unwrap_or(false);
            if !can_manage {
                bot.answer_callback_query(query.id.clone())
                    .text("Изменить автосбор может староста или администратор чата.")
                    .show_alert(true).await?;
                return Ok(true);
            }
            let result = sqlx::query(
                "UPDATE bot_chat_settings SET homework_autocollect_enabled=NOT homework_autocollect_enabled,updated_at=now() WHERE chat_id=$1 AND is_active=TRUE"
            ).bind(chat_id.0).execute(&state.db).await;
            if !matches!(result, Ok(ref r) if r.rows_affected()==1) {
                bot.answer_callback_query(query.id.clone()).text("Не удалось изменить настройку автосбора ДЗ. Сначала настрой учебный чат через /setgroup.").show_alert(true).await?;
                return Ok(true);
            }
        }
        "chatsettings:approver" => {
            let current: Option<String> = sqlx::query_scalar(
                "SELECT homework_approver_role FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE"
            ).bind(chat_id.0).fetch_optional(&state.db).await.ok().flatten();
            let next = match current.as_deref() {
                Some("class_leader") => "moderator",
                Some("moderator") => "admin",
                _ => "class_leader",
            };
            let result = sqlx::query(
                "UPDATE bot_chat_settings SET homework_approver_role=$2,updated_at=now() WHERE chat_id=$1 AND is_active=TRUE"
            ).bind(chat_id.0).bind(next).execute(&state.db).await;
            if !matches!(result, Ok(ref r) if r.rows_affected()==1) {
                bot.answer_callback_query(query.id.clone()).text("Не удалось изменить роль подтверждающего.").show_alert(true).await?;
                return Ok(true);
            }
        }
        _ => {
            let column = match action {
                "chatsettings:daily" => "daily_schedule_enabled",
                "chatsettings:lessons" => "lesson_notifications_enabled",
                "chatsettings:digest" => "daily_digest_enabled",
                _ => "",
            };
            if column.is_empty() {
                bot.answer_callback_query(query.id.clone()).text("Кнопка устарела.").await?;
                return Ok(true);
            }
            let sql = format!("UPDATE bot_chat_settings SET {column}=NOT {column},updated_at=now() WHERE chat_id=$1");
            let result = sqlx::query(&sql).bind(chat_id.0).execute(&state.db).await;
            if !matches!(result, Ok(ref r) if r.rows_affected()==1) {
                bot.answer_callback_query(query.id.clone()).text("Не удалось сохранить настройку.").show_alert(true).await?;
                return Ok(true);
            }
            let value: bool = sqlx::query_scalar(&format!("SELECT {column} FROM bot_chat_settings WHERE chat_id=$1"))
                .bind(chat_id.0).fetch_one(&state.db).await.unwrap_or(false);
            let _ = sqlx::query(
                "INSERT INTO bot_chat_assistant_settings_audit(chat_id,changed_by,setting,value) VALUES($1,$2,$3,to_jsonb($4::boolean))"
            ).bind(chat_id.0).bind(actor).bind(column).bind(value).execute(&state.db).await;
        }
    }

    let row = sqlx::query_as::<_, (String,bool,bool,bool,bool,bool,bool,String)>(
        "SELECT g.name,c.daily_schedule_enabled,c.lesson_notifications_enabled,c.daily_digest_enabled,c.assistant_enabled,c.context_enabled,c.homework_autocollect_enabled,c.homework_approver_role FROM bot_chat_settings c JOIN groups g ON g.id=c.group_id WHERE c.chat_id=$1 AND c.is_active=TRUE"
    ).bind(chat_id.0).fetch_optional(&state.db).await;
    let Ok(Some((group,daily,lessons,digest,assistant,context,autodz,approver)))=row else {
        bot.answer_callback_query(query.id.clone()).text("Сначала выбери группу командой /setgroup.").show_alert(true).await?;
        return Ok(true);
    };
    bot.answer_callback_query(query.id.clone()).await?;
    let text = format!(
        "⚙️ НАСТРОЙКИ УЧЕБНОГО ЧАТА\n\nГруппа: {group}\nЧасовой пояс: Москва (UTC+3)\n\nЕжедневная публикация: {}\nУведомления о парах: {}\nДайджест: {} · 07:00 МСК\nПомощник в ответах: {}\nКонтекст ответов: {}\n\nКонтекст собирается только при ответе на сообщение самого бота; он хранится отдельно для этого чата до 7 дней.",
        if daily {"включена"} else {"выключена"},
        if lessons {"включены"} else {"выключены"},
        if digest {"включён"} else {"выключен"},
        if assistant {"включён"} else {"выключен"},
        if context {"включён"} else {"выключен"},
    );
    bot.edit_message_text(chat_id,message.id,text)
        .reply_markup(chat_settings_keyboard(chat_id,&group,daily,lessons,digest,assistant,context,autodz,&approver))
        .await?;
    Ok(true)
}

async fn handle_ui_callback(
    bot: Bot,
    state: Arc<AppState>,
    query: CallbackQuery,
) -> ResponseResult<()> {
    let Some(data) = query.data.as_deref() else {
        bot.answer_callback_query(query.id.clone()).await?;
        return Ok(());
    };
    if handle_group_setup_callback(&bot, &state, &query).await? {
        return Ok(());
    }
    if handle_chat_settings_callback(&bot, &state, &query).await? {
        return Ok(());
    }
    if data.starts_with("pc:") || data.starts_with("dating:") {
        crate::services::student_social::handle_callback(
            &bot,
            &state.db,
            &query,
            state.config.admin_telegram_id,
        )
        .await?;
        return Ok(());
    }
    // Inline-mode callbacks carry inline_message_id instead of a regular chat
    // message. Use the actor's private chat as the personal context and edit the
    // inline message through editMessageTextInline at the end of this handler.
    let message = query.regular_message();
    let inline_message_id = query.inline_message_id.clone();
    if message.is_none() && inline_message_id.is_none() {
        bot.answer_callback_query(query.id.clone()).await?;
        return Ok(());
    }
    let actor = i64::try_from(query.from.id.0).unwrap_or_default();
    let chat_id = message.map(|value| value.chat.id.0).unwrap_or(actor);
    let settings = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT settings FROM users WHERE telegram_id=$1",
    )
    .bind(actor)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    let mut reminders_enabled = settings["reminders_enabled"] == true;
    let mut reminder_minutes = settings["reminder_minutes"].as_i64().unwrap_or(10);

    let action = data.to_owned();
    if action == "ban:yes" || action == "ban:no" {
        let Some(message) = message else {
            bot.answer_callback_query(query.id.clone())
                .text("Подтверждение модерации доступно только в сообщении учебного чата.")
                .show_alert(true)
                .await?;
            return Ok(());
        };
        let pending = sqlx::query_as::<_, (serde_json::Value, DateTime<Utc>)>(
            "SELECT payload,expires_at FROM bot_user_flows WHERE chat_id=$1 AND telegram_id=$2 AND flow='ban_confirm'"
        ).bind(chat_id).bind(actor).fetch_optional(&state.db).await.ok().flatten();
        let Some((payload, _)) = pending.filter(|(_, expires)| *expires > Utc::now()) else {
            bot.answer_callback_query(query.id.clone())
                .text("Подтверждение истекло. Запусти /ban ещё раз.")
                .show_alert(true)
                .await?;
            return Ok(());
        };
        let target = payload["target_id"].as_i64().unwrap_or_default();
        let reason = payload["reason"].as_str().unwrap_or("").to_owned();
        let _ = sqlx::query(
            "DELETE FROM bot_user_flows WHERE chat_id=$1 AND telegram_id=$2 AND flow='ban_confirm'",
        )
        .bind(chat_id)
        .bind(actor)
        .execute(&state.db)
        .await;
        if action == "ban:no" {
            bot.answer_callback_query(query.id.clone()).await?;
            bot.edit_message_text(message.chat.id, message.id, "Блокировка отменена.")
                .await?;
            return Ok(());
        }
        if chat_id >= 0
            || !crate::services::chat_roles::can_ban_chat(&bot, &state.db, message.chat.id, actor)
                .await
        {
            bot.answer_callback_query(query.id.clone())
                .text("Права модератора изменились; блокировка не выполнена.")
                .show_alert(true)
                .await?;
            return Ok(());
        }
        if let Err(error) = moderation_target_status(&bot, &state.db, message.chat.id, target).await
        {
            bot.answer_callback_query(query.id.clone())
                .text(error)
                .show_alert(true)
                .await?;
            return Ok(());
        }
        let logged = record_moderation_event(
            &state.db,
            message.chat.id,
            actor,
            target,
            "ban_requested",
            serde_json::json!({"reason":reason}),
        )
        .await
        .is_ok();
        let text = if logged
            && bot
                .ban_chat_member(message.chat.id, UserId(target as u64))
                .revoke_messages(false)
                .await
                .is_ok()
        {
            let _ = record_moderation_event(
                &state.db,
                message.chat.id,
                actor,
                target,
                "ban_applied",
                serde_json::json!({"reason":reason}),
            )
            .await;
            format!("⛔ Участник {target} заблокирован. Для разблокировки: /unban {target}")
        } else {
            "Telegram не применил блокировку или журнал недоступен. Проверь права бота и повтори попытку.".into()
        };
        bot.answer_callback_query(query.id.clone()).await?;
        bot.edit_message_text(message.chat.id, message.id, text)
            .await?;
        return Ok(());
    }
    if let Some(rest) = action.strip_prefix("dzdel:") {
        let Some(message) = message else {
            bot.answer_callback_query(query.id.clone())
                .text("Удаление задания подтверждается в сообщении учебного чата.")
                .show_alert(true)
                .await?;
            return Ok(());
        };
        let mut parts = rest.split(':');
        let id = parts
            .next()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|v| *v > 0);
        let confirm = parts.next() == Some("yes");
        let Some(id) = id else {
            bot.answer_callback_query(query.id.clone())
                .text("Некорректный номер задания.")
                .show_alert(true)
                .await?;
            return Ok(());
        };
        if !confirm {
            bot.answer_callback_query(query.id.clone()).await?;
            bot.edit_message_text(message.chat.id, message.id, "Удаление отменено.")
                .await?;
            return Ok(());
        }
        if !crate::services::group_content::can_edit_homework(&state.db, chat_id, actor).await {
            bot.answer_callback_query(query.id.clone())
                .text("Удалить задание может только пользователь с правом редактирования ДЗ.")
                .show_alert(true)
                .await?;
            return Ok(());
        }
        let result = sqlx::query("UPDATE bot_homework SET status='archived',updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'")
            .bind(chat_id).bind(id).execute(&state.db).await;
        let text = match result {
            Ok(r) if r.rows_affected() == 1 => {
                format!("🗄 Задание #{id} убрано из активного списка.")
            }
            Ok(_) => "Задание уже удалено или не найдено.".into(),
            Err(_) => "Не удалось удалить задание.".into(),
        };
        let _ = sqlx::query("INSERT INTO bot_group_content_audit(chat_id,actor_telegram_id,action,record_type,record_id,details) VALUES($1,$2,'delete','homework',$3,'{}'::jsonb)")
            .bind(chat_id).bind(actor).bind(id).execute(&state.db).await;
        bot.answer_callback_query(query.id.clone()).await?;
        bot.edit_message_text(message.chat.id, message.id, text)
            .await?;
        return Ok(());
    }
    match crate::services::homework_candidates::handle_callback(&bot, &state.db, &query).await {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(error) => {
            tracing::warn!("Homework candidate callback failed: {}", error);
            bot.answer_callback_query(query.id.clone())
                .text("Не удалось обработать действие. Обнови меню и попробуй снова.")
                .show_alert(true)
                .await?;
            return Ok(());
        }
    }
    let mut screen = "main";
    let text: String;
    match action.as_str() {
        "ui:onboarding:1" => {
            screen = "onboarding-1";
            text = "Шаг 1 из 3 · выбери учебную группу через /group или кнопку Mini App.".into();
        }
        "ui:onboarding:2" => {
            screen = "onboarding-2";
            text =
                "Шаг 2 из 3 · включи личные напоминания в настройках и выбери удобный срок.".into();
        }
        "ui:onboarding:3" => {
            screen = "onboarding-3";
            text = "Шаг 3 из 3 · основные разделы доступны кнопками внизу, дополнительные действия — здесь.".into();
        }
        "ui:onboarding:done" => {
            if chat_id != actor {
                bot.answer_callback_query(query.id.clone())
                    .text("Открой знакомство в личном чате с ботом.")
                    .show_alert(true)
                    .await?;
                return Ok(());
            }
            if let Err(error) = sqlx::query("UPDATE users SET settings=jsonb_set(settings,'{onboarding_completed}','true'::jsonb,true),updated_at=now() WHERE telegram_id=$1")
                .bind(actor)
                .execute(&state.db)
                .await
            {
                tracing::warn!("Could not save onboarding completion: {}", error);
                bot.answer_callback_query(query.id.clone())
                    .text("Не удалось сохранить завершение знакомства. Попробуй ещё раз.")
                    .show_alert(true)
                    .await?;
                return Ok(());
            }
            text = "Главное меню · выбери, что нужно сейчас.".into();
        }
        "ui:main" => text = "Главное меню · выбери раздел.".into(),
        "ui:schedule" => {
            screen = "schedule";
            text = "Расписание · выбери период.".into();
        }
        "ui:schedule:today" | "ui:schedule:tomorrow" | "ui:schedule:week" | "ui:schedule:next" => {
            screen = "schedule";
            let command = match action.as_str() {
                "ui:schedule:today" => "/today",
                "ui:schedule:tomorrow" => "/tomorrow",
                "ui:schedule:week" => "/week",
                _ => "/next",
            };
            text = match selected_group(&state.db, actor, chat_id).await {
                Some(group) => schedule_text(&state.db, group, command)
                    .await
                    .replace('*', ""),
                None => "Сначала выбери группу: /group ИВТ-21".into(),
            };
        }
        "ui:home" => {
            text = crate::services::student_tools::handle(
                &state.db, chat_id, actor, "/home", "", None, None,
            )
            .await
            .unwrap_or_default()
        }
        "ui:homework" => {
            screen = "homework";
            if chat_id < 0 {
                text = crate::services::group_content::handle_homework(
                    &state.db, chat_id, actor, "/dz", "",
                )
                .await
                .unwrap_or_else(|| "Домашние задания доступны в учебной группе.".into());
            } else {
                text = match selected_group(&state.db, actor, chat_id).await {
                    Some(group_id) => crate::services::group_content::homework_for_group(
                        &state.db, group_id, actor,
                    )
                    .await,
                    None => "Сначала выбери учебную группу: /group название. После этого здесь появятся задания, опубликованные в привязанном учебном чате.".into(),
                };
            }
        }
        "ui:deadlines" => {
            screen = "deadlines";
            text = crate::services::student_tools::handle(
                &state.db,
                chat_id,
                actor,
                "/deadlines",
                "",
                None,
                None,
            )
            .await
            .unwrap_or_default();
        }
        "ui:teacher" => {
            text = "Напиши фамилию преподавателя после команды /teacher, например: /teacher Иванов"
                .into()
        }
        "ui:rooms" => {
            text = crate::services::student_tools::handle(
                &state.db,
                chat_id,
                actor,
                "/free_rooms",
                "",
                None,
                None,
            )
            .await
            .unwrap_or_default()
        }
        "ui:settings" => {
            if chat_id != actor {
                screen = "help";
                text = match sqlx::query_as::<_, (String, bool, String, bool, bool, Option<i64>, String)>(
                    r#"
                    SELECT g.name, c.daily_schedule_enabled,
                           to_char(c.daily_schedule_time, 'HH24:MI'),
                           c.lesson_notifications_enabled, c.is_active,
                           c.message_thread_id, c.timezone
                    FROM bot_chat_settings c
                    JOIN groups g ON g.id = c.group_id
                    WHERE c.chat_id = $1
                    "#,
                )
                .bind(chat_id)
                .fetch_optional(&state.db)
                .await
                {
                    Ok(Some((group, daily, time, notifications, active, thread_id, timezone))) => format!(
                        "⚙️ Настройки учебного чата\nГруппа: {group}\nЧасовой пояс: {timezone}\nАвтопубликация: {} (ежедневно в {time})\nУведомления о парах: {}\nТема публикаций: {}\nИнтеграция: {}\n\nКоманды администратора:\n/chat_timezone Europe/Helsinki — часовой пояс\n/chat_topic — выбрать текущую тему\n/chat_topic_off — публиковать в общий чат\n/daily_on — включить расписание каждый день\n/daily_off — выключить\n/daily_time ЧЧ:ММ — время публикации\n/chat_notify_on — уведомлять о начале пар\n/chat_notify_off — выключить",
                        if daily { "включена" } else { "выключена" },
                        if notifications { "включены" } else { "выключены" },
                        thread_id.map(|id| format!("тема #{}", id)).unwrap_or_else(|| "общий чат".into()),
                        if active { "активна" } else { "приостановлена" }
                    ),
                    Ok(None) => "Учебный чат ещё не настроен. Администратор может привязать группу командой /setgroup название.".into(),
                    Err(error) => {
                        tracing::warn!("Could not load group settings from menu: {}", error);
                        "Не удалось загрузить настройки учебного чата. Повтори попытку через /chat_settings.".into()
                    }
                };
            } else {
                screen = "settings";
                text = format!("Настройки уведомлений\n\nНапоминания о паре: {} · за {} мин\nЕжедневный дайджест (07:00 МСК): {}\nИзменения расписания: {}",
                if reminders_enabled { "вкл" } else { "выкл" }, reminder_minutes,
                if settings["daily_digest_enabled"] == true { "вкл" } else { "выкл" },
                if settings["schedule_changes_enabled"] == true { "вкл" } else { "выкл" });
            }
        }
        "ui:reminders:toggle" => {
            if chat_id != actor {
                bot.answer_callback_query(query.id.clone())
                    .text("Личные уведомления настраиваются в личном чате с ботом.")
                    .show_alert(true)
                    .await?;
                return Ok(());
            }
            reminders_enabled = !reminders_enabled;
            if let Err(error) = sqlx::query("UPDATE users SET settings=jsonb_set(settings,'{reminders_enabled}',to_jsonb($2::boolean),true),updated_at=now() WHERE telegram_id=$1")
                .bind(actor).bind(reminders_enabled).execute(&state.db).await
            {
                tracing::warn!("Could not save lesson reminder toggle: {}", error);
                bot.answer_callback_query(query.id.clone())
                    .text("Не удалось сохранить настройку напоминаний. Попробуй ещё раз.")
                    .show_alert(true)
                    .await?;
                return Ok(());
            }
            screen = "settings";
            text = format!(
                "Настройки уведомлений\n\nНапоминания: {}\nВремя до пары: {} минут",
                if reminders_enabled {
                    "включены"
                } else {
                    "выключены"
                },
                reminder_minutes
            );
        }
        "ui:reminder:10" | "ui:reminder:60" | "ui:reminder:180" | "ui:reminder:1440" => {
            if chat_id != actor {
                bot.answer_callback_query(query.id.clone())
                    .text("Личные уведомления настраиваются в личном чате с ботом.")
                    .show_alert(true)
                    .await?;
                return Ok(());
            }
            reminder_minutes = action
                .rsplit(':')
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10);
            if let Err(error) = sqlx::query("UPDATE users SET settings=jsonb_set(settings,'{reminder_minutes}',to_jsonb($2::integer),true),updated_at=now() WHERE telegram_id=$1")
                .bind(actor).bind(reminder_minutes as i32).execute(&state.db).await
            {
                tracing::warn!("Could not save lesson reminder interval: {}", error);
                bot.answer_callback_query(query.id.clone())
                    .text("Не удалось сохранить интервал напоминания. Попробуй ещё раз.")
                    .show_alert(true)
                    .await?;
                return Ok(());
            }
            screen = "settings";
            text = format!(
                "Настройки уведомлений\n\nНапоминания: {}\nВремя до пары: {} минут",
                if reminders_enabled {
                    "включены"
                } else {
                    "выключены"
                },
                reminder_minutes
            );
        }
        "ui:digest:toggle" => {
            if chat_id != actor {
                bot.answer_callback_query(query.id.clone())
                    .text("Личные уведомления настраиваются в личном чате.")
                    .show_alert(true)
                    .await?;
                return Ok(());
            }
            let enabled = settings["daily_digest_enabled"] != true;
            let save_result = sqlx::query(
                "UPDATE users SET settings=jsonb_set(jsonb_set(settings,'{daily_digest_enabled}',to_jsonb($2::boolean),true),'{daily_digest_time}','\"07:00\"'::jsonb,true),updated_at=now() WHERE telegram_id=$1"
            ).bind(actor).bind(enabled).execute(&state.db).await;
            if let Err(error) = save_result {
                tracing::warn!("Could not save daily digest settings: {}", error);
                bot.answer_callback_query(query.id.clone())
                    .text("Не удалось сохранить настройки дайджеста. Попробуй ещё раз.")
                    .show_alert(true)
                    .await?;
                return Ok(());
            }
            screen = "settings";
            let settings = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT settings FROM users WHERE telegram_id=$1",
            )
            .bind(actor)
            .fetch_one(&state.db)
            .await
            .unwrap_or_default();
            text = format!("Настройки уведомлений\n\nНапоминания о паре: {} · за {} мин\nЕжедневный дайджест: {} · {}\nИзменения расписания: {}",
                if reminders_enabled { "вкл" } else { "выкл" }, reminder_minutes,
                if settings["daily_digest_enabled"] == true { "вкл" } else { "выкл" }, settings["daily_digest_time"].as_str().unwrap_or("07:00"),
                if settings["schedule_changes_enabled"] == true { "вкл" } else { "выкл" });
        }
        "ui:changes:toggle" => {
            if chat_id != actor {
                bot.answer_callback_query(query.id.clone())
                    .text("Личные уведомления настраиваются в личном чате.")
                    .show_alert(true)
                    .await?;
                return Ok(());
            }
            let enabled = settings["schedule_changes_enabled"] != true;
            if let Err(error) = sqlx::query("UPDATE users SET settings=jsonb_set(settings,'{schedule_changes_enabled}',to_jsonb($2::boolean),true),updated_at=now() WHERE telegram_id=$1")
                .bind(actor).bind(enabled).execute(&state.db).await
            {
                tracing::warn!("Could not save schedule change notification setting: {}", error);
                bot.answer_callback_query(query.id.clone())
                    .text("Не удалось сохранить настройку изменений расписания. Попробуй ещё раз.")
                    .show_alert(true)
                    .await?;
                return Ok(());
            }
            screen = "settings";
            let refreshed = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT settings FROM users WHERE telegram_id=$1",
            )
            .bind(actor)
            .fetch_one(&state.db)
            .await
            .unwrap_or_default();
            text = format!("Настройки уведомлений\n\nНапоминания о паре: {} · за {} мин\nЕжедневный дайджест: {} · {}\nИзменения расписания: {}",
                if reminders_enabled { "вкл" } else { "выкл" }, reminder_minutes,
                if refreshed["daily_digest_enabled"] == true { "вкл" } else { "выкл" }, refreshed["daily_digest_time"].as_str().unwrap_or("07:00"),
                if enabled { "вкл" } else { "выкл" });
        }
        "ui:commands" => {
            screen = "help";
            text = commands_text(chat_id < 0);
        }
        "ui:help" => {
            screen = "help";
            text = "Помощь\n\nРасписание и ДЗ доступны кнопками. Группу выбери командой /group. Для поиска преподавателя используй /teacher фамилия. Аудитории можно проверить кнопкой в меню. Полный список команд — кнопка ниже или /commands.".into();
        }
        _ => {
            bot.answer_callback_query(query.id.clone())
                .text("Эта кнопка устарела. Открой /menu заново.")
                .show_alert(true)
                .await?;
            return Ok(());
        }
    }

    // Telegram's callback spinner is always dismissed before editing the screen.
    bot.answer_callback_query(query.id.clone()).await?;
    let keyboard = ui_keyboard(screen, reminders_enabled, reminder_minutes);
    // The regular edit endpoint returns Message, while the inline edit
    // endpoint returns True. Normalize both successful responses to () so the
    // branches have the same Result type.
    let edit_result = if let Some(message) = message {
        bot.edit_message_text(message.chat.id, message.id, text)
            .reply_markup(keyboard)
            .await
            .map(|_| ())
    } else if let Some(inline_message_id) = inline_message_id.as_deref() {
        bot.edit_message_text_inline(inline_message_id, text)
            .reply_markup(keyboard)
            .await
            .map(|_| ())
    } else {
        return Ok(());
    };
    match edit_result {
        Ok(_) => {}
        // A repeated tap may lead to the same screen and markup. That is
        // already the desired state, not a broken callback.
        Err(error) if error.to_string().contains("message is not modified") => {}
        Err(error) => return Err(error),
    }
    Ok(())
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
    // Make bot answers actual Telegram replies so users can keep a readable
    // thread around a homework post, schedule post, or a previous question.
    let is_schedule = text.contains("*РАСПИСАНИЕ") || text.contains("*СЛЕДУЮЩАЯ ПАРА*");
    let request = bot.send_message(message.chat.id, text).reply_parameters(ReplyParameters::new(message.id));
    let request = if is_schedule {
        request.parse_mode(ParseMode::Markdown)
    } else {
        request
    };
    let request = if message.chat.is_private() {
        request.reply_markup(main_keyboard(webapp_url, true))
    } else {
        request
    };
    let request = match message.thread_id.clone() {
        Some(thread_id) => request.message_thread_id(thread_id),
        None => request,
    };
    let sent = request.await?;

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

async fn maybe_answer_bot_reply(
    bot: &Bot,
    db: &PgPool,
    webapp_url: &str,
    message: &Message,
    actor: i64,
    text: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if text.trim().starts_with('/') || text.chars().count() > 4000 {
        return Ok(false);
    }
    let Some(replied) = message.reply_to_message() else {
        return Ok(false);
    };
    let chat_id = message.chat.id.0;
    let source_message_id = i64::from(replied.id.0);
    let is_bot_message: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bot_messages WHERE chat_id=$1 AND message_id=$2)"
    ).bind(chat_id).bind(replied.id.0).fetch_one(db).await?;
    if !is_bot_message {
        return Ok(false);
    }

    let is_group_chat = chat_id < 0;
    let settings = if is_group_chat {
        sqlx::query_as::<_, (Option<uuid::Uuid>, bool, bool, i16)>(
            "SELECT group_id,assistant_enabled,context_enabled,context_retention_days FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE"
        ).bind(chat_id).fetch_optional(db).await?
    } else {
        // Private conversations use the account's linked group but never persist
        // reply history; the quoted bot message is sufficient for one-turn context.
        Some((selected_group(db, actor, chat_id).await, true, false, 14_i16))
    };
    let Some((group_id, assistant_enabled, context_enabled, retention_days)) = settings else {
        return Ok(false);
    };
    if !assistant_enabled {
        return Ok(false);
    }

    let original = replied.text().or_else(|| replied.caption()).unwrap_or("[сообщение бота без текста]");
    let original: String = original.chars().take(4000).collect();
    let user_text: String = text.trim().chars().take(4000).collect();
    // Saving chat context is optional and remains disabled without admin consent.
    // A direct reply can still use the quoted bot message for this one answer.
    let history = if context_enabled {
        let mut tx = db.begin().await?;
        sqlx::query(
            "INSERT INTO bot_chat_context_messages(chat_id,message_id,sender_role,message_kind,content) VALUES($1,$2,'bot','text',$3) ON CONFLICT(chat_id,message_id) DO NOTHING"
        ).bind(chat_id).bind(source_message_id).bind(&original).execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO bot_chat_context_messages(chat_id,message_id,reply_to_message_id,sender_telegram_id,sender_role,message_kind,content) VALUES($1,$2,$3,$4,'member','text',$5) ON CONFLICT(chat_id,message_id) DO NOTHING"
        ).bind(chat_id).bind(i64::from(message.id.0)).bind(source_message_id).bind(actor).bind(&user_text).execute(&mut *tx).await?;

        sqlx::query(
            "DELETE FROM bot_chat_context_messages WHERE chat_id=$1 AND created_at < now() - make_interval(days => $2::int)"
        ).bind(chat_id).bind(i32::from(retention_days)).execute(&mut *tx).await?;
        sqlx::query(
            "DELETE FROM bot_chat_context_messages WHERE chat_id=$1 AND id IN (SELECT id FROM bot_chat_context_messages WHERE chat_id=$1 ORDER BY created_at DESC,id DESC OFFSET 100)"
        ).bind(chat_id).execute(&mut *tx).await?;
        let rows = sqlx::query_as::<_, (String,String)>(
            "SELECT sender_role,content FROM bot_chat_context_messages WHERE chat_id=$1 ORDER BY created_at DESC,id DESC LIMIT 10"
        ).bind(chat_id).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        rows
    } else {
        Vec::new()
    };
    let previous_bot_text = history
        .iter()
        .find(|(role, _)| role == "bot")
        .map(|(_, content)| content.as_str())
        .unwrap_or(original.as_str());

    let mut intent = crate::services::conversation::detect(&user_text);
    if intent.is_none() {
        let lower = user_text.to_lowercase().replace("ё","е");
        let follow_up = lower.starts_with("а ") || lower.starts_with("и ") || lower.starts_with("почему ")
            || lower.starts_with("а если") || lower.starts_with("а когда");
        if follow_up {
            if !previous_bot_text.is_empty() {
                let topic = previous_bot_text.to_lowercase();
                if topic.contains("расписан") || topic.contains("пара") {
                    intent = Some(crate::services::conversation::Intent::Tomorrow);
                } else if topic.contains("домаш") || topic.contains("задан") || topic.contains("дз") {
                    intent = Some(
                        crate::services::conversation::homework_subject(&lower)
                            .map(crate::services::conversation::Intent::HomeworkFor)
                            .unwrap_or(crate::services::conversation::Intent::Homework)
                    );
                } else if topic.contains("дедлайн") || topic.contains("срок") {
                    intent = Some(crate::services::conversation::Intent::Deadlines);
                }
            }
        }
    }

    let answer = match intent {
        Some(crate::services::conversation::Intent::Greeting) => Some(
            "Привет! Я могу помочь с расписанием, ДЗ, дедлайнами, преподавателями и аудиториями.".to_owned()
        ),
        Some(crate::services::conversation::Intent::Help) => Some(commands_text(true)),
        Some(crate::services::conversation::Intent::Today) => {
            match group_id {
                Some(group) => Some(schedule_text(db, group, "/today").await),
                None => None,
            }
        },
        Some(crate::services::conversation::Intent::Tomorrow) => {
            match group_id {
                Some(group) => Some(schedule_text(db, group, "/tomorrow").await),
                None => None,
            }
        },
        Some(crate::services::conversation::Intent::Week) => {
            match group_id {
                Some(group) => Some(schedule_text(db, group, "/week").await),
                None => None,
            }
        },
        Some(crate::services::conversation::Intent::NextLesson) => {
            match group_id {
                Some(group) => Some(schedule_text(db, group, "/next").await),
                None => None,
            }
        },
        Some(crate::services::conversation::Intent::Homework) => {
            if let Some(group) = group_id {
                Some(crate::services::group_content::homework_for_group(db, group, actor).await)
            } else {
                Some("Сначала администратор должен привязать учебную группу к этому чату командой /setgroup.".into())
            }
        }
        Some(crate::services::conversation::Intent::HomeworkFor(subject)) => {
            if let Some(group) = group_id {
                Some(crate::services::group_content::homework_for_group_subject(db, group, actor, &subject).await)
            } else {
                Some("Сначала администратор должен привязать учебную группу к этому чату командой /setgroup.".into())
            }
        }
        Some(crate::services::conversation::Intent::Deadlines) => {
            crate::services::student_tools::handle(db, chat_id, actor, "/deadlines", "", None, None).await
        }
        Some(crate::services::conversation::Intent::FreeRooms(time)) => {
            crate::services::student_tools::handle(db, chat_id, actor, "/free_rooms", time.as_deref().unwrap_or(""), None, None).await
        }
        Some(crate::services::conversation::Intent::Teacher(query)) => {
            crate::services::student_tools::handle(db, chat_id, actor, "/teacher", &query, None, None).await
        }
        None => {
            let excerpt: String = previous_bot_text.chars().take(180).collect();
            let fallback = if excerpt.is_empty() {
                "Я не распознал тему вопроса. Могу помочь с расписанием, ДЗ, дедлайнами, преподавателями и аудиториями — уточни, что именно нужно.".to_owned()
            } else {
                format!("Продолжаем тему сообщения «{excerpt}». Уточни, какую информацию нужно найти — я умею искать расписание, ДЗ, дедлайны, преподавателей и аудитории.")
            };
            let llm_history = history.iter().rev().take(8).map(|(role, content)| {
                let role = if role == "bot" { "assistant" } else { "user" };
                (role.to_owned(), content.clone())
            }).collect::<Vec<_>>();
            Some(crate::services::ollama::chat(&state.config, &user_text, &llm_history)
                .await.unwrap_or(fallback))
        }
    };

    let answer = answer.unwrap_or_else(|| {
        "Для этого вопроса нужна привязанная учебная группа. Администратор может настроить её командой /setgroup.".into()
    });
    reply(bot, db, message, answer, webapp_url)
        .await
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)?;
    Ok(true)
}

async fn parse_target_and_tail(
    db: &PgPool,
    chat_id: i64,
    message: &Message,
    argument: &str,
) -> Result<(i64, String), String> {
    if let Some(user) = message
        .reply_to_message()
        .and_then(|replied| replied.from.as_ref())
    {
        let target_id = i64::try_from(user.id.0)
            .map_err(|_| "Некорректный Telegram ID участника".to_owned())?;
        return Ok((target_id, argument.trim().to_owned()));
    }

    let mut parts = argument.splitn(2, char::is_whitespace);
    let target = parts.next().unwrap_or_default();
    let tail = parts.next().unwrap_or_default().trim().to_owned();
    let target_id = if let Ok(id) = target.parse::<i64>() {
        if id <= 0 {
            return Err("Telegram ID должен быть положительным числом.".into());
        }
        id
    } else {
        let username = target.trim_start_matches('@');
        if chat_id >= 0
            || !(5..=32).contains(&username.len())
            || !username
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(
                "Укажи @username участника, Telegram ID или ответь на его сообщение.".into(),
            );
        }
        sqlx::query_scalar::<_, i64>(
            "SELECT telegram_id FROM bot_chat_members WHERE chat_id=$1 AND lower(username)=lower($2) AND telegram_status NOT IN ('left','kicked')"
        ).bind(chat_id).bind(username).fetch_optional(db).await.ok().flatten().ok_or_else(||
            format!("Не нашёл @{username} среди известных участников. Попроси его написать сообщение в группу, затем повтори команду.")
        )?
    };
    Ok((target_id, tail))
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

async fn set_member_command_menu(bot: &Bot, chat_id: ChatId, target_id: i64, role: &str) {
    let mut commands = vec![
        BotCommand::new("start", "Краткое знакомство"),
        BotCommand::new("help", "Помощь"),
        BotCommand::new("menu", "Главное меню"),
        BotCommand::new("commands", "Показать список команд"),
        BotCommand::new("settings", "Настройки"),
        BotCommand::new("today", "Расписание на сегодня"),
        BotCommand::new("tomorrow", "Расписание на завтра"),
        BotCommand::new("week", "Расписание на 7 дней"),
        BotCommand::new("next", "Следующая пара"),
        BotCommand::new("homework", "Домашние задания"),
        BotCommand::new("deadlines", "Ближайшие сроки"),
        BotCommand::new("teacher", "Преподаватель"),
        BotCommand::new("free_rooms", "Свободные аудитории"),
        BotCommand::new("resources", "Материалы группы"),
        BotCommand::new("report", "Пожаловаться на сообщение"),
        BotCommand::new("lost_found", "Потерянные вещи"),
    ];
    if matches!(
        role,
        "class_leader" | "moderator" | "senior_moderator" | "helper"
    ) {
        commands.extend([
            BotCommand::new("add_dz", "Добавить задание"),
            BotCommand::new("edit_dz", "Изменить задание"),
            BotCommand::new("autodz_on", "Включить сбор предложений ДЗ"),
            BotCommand::new("autodz_off", "Выключить сбор предложений ДЗ"),
            BotCommand::new("subject_aliases", "Сокращения предметов"),
            BotCommand::new("poll", "Создать опрос группы"),
        ]);
    }
    if matches!(role, "moderator" | "senior_moderator" | "helper") {
        commands.extend([
            BotCommand::new("report_queue", "Открытые жалобы"),
            BotCommand::new("report_resolve", "Закрыть жалобу"),
            BotCommand::new("warnings", "История предупреждений"),
            BotCommand::new("warn", "Предупредить участника"),
            BotCommand::new("mute", "Ограничить участника"),
            BotCommand::new("unmute", "Снять мут"),
        ]);
    }
    let Ok(user_id) = u64::try_from(target_id) else {
        return;
    };
    let scope = BotCommandScope::ChatMember {
        chat_id: Recipient::Id(chat_id),
        user_id: UserId(user_id),
    };
    if let Err(error) = bot
        .set_my_commands(commands)
        .scope(scope)
        .language_code("ru")
        .await
    {
        tracing::warn!("Could not set role-specific command menu: {}", error);
    }
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
        WHERE is_active = true AND name ILIKE $2
        ORDER BY CASE WHEN lower(name) = lower($1) THEN 0 ELSE 1 END, name
        LIMIT 1
        "#,
    )
    .bind(argument.trim())
    .bind(format!("%{}%", argument.trim()))
    .fetch_optional(db)
    .await
}

async fn selected_group(db: &PgPool, telegram_id: i64, chat_id: i64) -> Option<uuid::Uuid> {
    if chat_id < 0 {
        sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT group_id FROM bot_chat_settings WHERE chat_id = $1 AND is_active = TRUE",
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
    let today = local_now.date_naive();
    match command {
        "/today" | "📅 Сегодня" => {
            let tomorrow = today + ChronoDuration::days(1);
            let from = moscow_offset().from_local_datetime(&today.and_hms_opt(0,0,0).unwrap()).single().unwrap().with_timezone(&Utc);
            let to = moscow_offset().from_local_datetime(&tomorrow.and_hms_opt(0,0,0).unwrap()).single().unwrap().with_timezone(&Utc);
            (from, to)
        }
        "/tomorrow" | "📅 Завтра" => {
            let tomorrow = today + ChronoDuration::days(1);
            let day_after = today + ChronoDuration::days(2);
            let from = moscow_offset().from_local_datetime(&tomorrow.and_hms_opt(0,0,0).unwrap()).single().unwrap().with_timezone(&Utc);
            let to = moscow_offset().from_local_datetime(&day_after.and_hms_opt(0,0,0).unwrap()).single().unwrap().with_timezone(&Utc);
            (from, to)
        }
        "/week" | "🗓 Неделя" => (now, now + ChronoDuration::days(7)),
        _ => (now, now + ChronoDuration::days(30)),
    }
}

type ScheduleRow = (String, DateTime<Utc>, DateTime<Utc>, Option<String>, Option<String>);

fn moscow_midnight_utc(date: NaiveDate) -> DateTime<Utc> {
    moscow_offset()
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("valid midnight"))
        .single()
        .expect("UTC+3 has no daylight-saving gap")
        .with_timezone(&Utc)
}

fn weekday_ru(date: NaiveDate) -> &'static str {
    match date.weekday().num_days_from_monday() {
        0 => "понедельник",
        1 => "вторник",
        2 => "среда",
        3 => "четверг",
        4 => "пятница",
        5 => "суббота",
        _ => "воскресенье",
    }
}

async fn load_schedule_rows(
    db: &PgPool,
    group_id: uuid::Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    include_ongoing: bool,
) -> Vec<ScheduleRow> {
    let query = if include_ongoing {
        sqlx::query_as::<_, ScheduleRow>(
            r#"
            SELECT l.subject,l.starts_at,l.ends_at,r.name,t.full_name
            FROM lessons l
            LEFT JOIN rooms r ON r.id=l.room_id
            LEFT JOIN teachers t ON t.id=l.teacher_id
            WHERE l.group_id=$1 AND l.ends_at>$2 AND l.starts_at<$3
            ORDER BY l.starts_at
            LIMIT 30
            "#,
        )
        .bind(group_id)
        .bind(from)
        .bind(to)
    } else {
        sqlx::query_as::<_, ScheduleRow>(
            r#"
            SELECT l.subject,l.starts_at,l.ends_at,r.name,t.full_name
            FROM lessons l
            LEFT JOIN rooms r ON r.id=l.room_id
            LEFT JOIN teachers t ON t.id=l.teacher_id
            WHERE l.group_id=$1 AND l.starts_at>=$2 AND l.starts_at<$3
            ORDER BY l.starts_at
            LIMIT 30
            "#,
        )
        .bind(group_id)
        .bind(from)
        .bind(to)
    };
    query.fetch_all(db).await.unwrap_or_default()
}

fn escape_schedule_markdown(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('_', "\\_")
        .replace('*', "\\*")
        .replace('`', "\\`")
        .replace('[', "\\[")
}

fn schedule_room_label(room: Option<String>) -> String {
    let room = room.unwrap_or_else(|| "Аудитория не указана".into());
    let lower = room.to_lowercase();
    if lower.contains("ауд")
        || lower.contains("онлайн")
        || lower.contains("дистант")
        || lower.contains("zoom")
        || lower.contains("teams")
    {
        room
    } else {
        format!("ауд. {room}")
    }
}

pub async fn schedule_text(db: &PgPool, group_id: uuid::Uuid, command: &str) -> String {
    let now = Utc::now();
    let local_now = now.with_timezone(&moscow_offset());
    let is_today = command == "/today" || command == "📅 Сегодня";
    let is_tomorrow = command == "/tomorrow" || command == "📅 Завтра";
    let (from, to) = period(command);
    let mut rows = load_schedule_rows(db, group_id, if is_today { now } else { from }, to, is_today).await;
    let mut nearest_day_label: Option<String> = None;

    // If today's remaining lessons are over, or tomorrow is a non-study day,
    // find the next actual date with lessons instead of showing an empty weekend.
    if rows.is_empty() && (is_today || is_tomorrow) {
        let start_day = local_now.date_naive() + ChronoDuration::days(if is_tomorrow { 2 } else { 1 });
        let search_from = moscow_midnight_utc(start_day);
        let nearest: Option<NaiveDate> = sqlx::query_scalar(
            "SELECT min((starts_at AT TIME ZONE 'Europe/Moscow')::date) FROM lessons WHERE group_id=$1 AND starts_at >= $2 AND starts_at < $2 + interval '30 days'"
        ).bind(group_id).bind(search_from).fetch_one(db).await.unwrap_or(None);
        if let Some(date) = nearest {
            let day_from = moscow_midnight_utc(date);
            let day_to = moscow_midnight_utc(date + ChronoDuration::days(1));
            rows = load_schedule_rows(db, group_id, day_from, day_to, false).await;
            if !rows.is_empty() {
                nearest_day_label = Some(format!("{} · {}", weekday_ru(date), date.format("%d.%m.%Y")));
            }
        }
    }

    if rows.is_empty() {
        let message = if is_today {
            "📭 *РАСПИСАНИЕ НА СЕГОДНЯ*\n\nНа сегодня пары уже закончились или занятий нет. На ближайшие 30 дней новых занятий не найдено."
        } else if is_tomorrow {
            "📭 *РАСПИСАНИЕ НА ЗАВТРА*\n\nНа завтра занятий нет, и ближайший учебный день не найден в следующих 30 днях."
        } else if command == "/next" || command == "⏭ Следующая пара" {
            "⏭ *СЛЕДУЮЩАЯ ПАРА*\n\nНа ближайшие 30 дней занятий не найдено."
        } else if command == "/week" || command == "🗓 Неделя" {
            "📭 *РАСПИСАНИЕ НА 7 ДНЕЙ*\n\nНа ближайшие семь дней занятий не найдено."
        } else {
            "📭 *РАСПИСАНИЕ*\n\nНа выбранный период занятий не найдено."
        };
        return format!("{}\nВозможно, расписание ещё не обновилось.", message);
    }

    let is_next = command == "/next" || command == "⏭ Следующая пара";
    let selected = if is_next { rows.into_iter().take(1).collect::<Vec<_>>() } else { rows };
    let heading = if let Some(label) = nearest_day_label {
        format!("📅 *РАСПИСАНИЕ НА БЛИЖАЙШИЙ УЧЕБНЫЙ ДЕНЬ*\n{label}")
    } else {
        match command {
            "/today" | "📅 Сегодня" => "📅 *РАСПИСАНИЕ НА СЕГОДНЯ*".to_owned(),
            "/tomorrow" | "📅 Завтра" => "📅 *РАСПИСАНИЕ НА ЗАВТРА*".to_owned(),
            "/next" | "⏭ Следующая пара" => "⏭ *СЛЕДУЮЩАЯ ПАРА*".to_owned(),
            "/week" | "🗓 Неделя" => "🗓 *РАСПИСАНИЕ НА 7 ДНЕЙ*".to_owned(),
            _ => "🎓 *РАСПИСАНИЕ*".to_owned(),
        }
    };
    let mut output = format!("{}\n\n", heading);
    if is_today && selected.iter().any(|(_, start, end, _, _)| *start <= now && *end > now) {
        output.push_str("Сейчас идёт пара. Ниже показаны текущая и оставшиеся пары.\n");
    }
    for (subject, start, end, room, teacher) in selected {
        output.push_str(&format!(
            "━━━━━━━━━━━━━━\n📚 *{}*\n🕒 {}–{}\n📍 {}\n👨‍🏫 {}\n",
            escape_schedule_markdown(&subject),
            moscow_time(start, "%d.%m %H:%M"),
            moscow_time(end, "%H:%M"),
            escape_schedule_markdown(&schedule_room_label(room)),
            escape_schedule_markdown(&teacher.unwrap_or_else(|| "Преподаватель не указан".into())),
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
        "next"
        | "пара"
        | "следующая"
        | "следующая пара"
        | "ближайшая пара"
        | "следующую"
        | "/next" => vec![INLINE_SCHEDULE_ACTIONS[2]],
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
        // Dating profile photos arrive without message.text(). Give the active
        // private photo flow first refusal before the generic attachment handler.
        if message.photo().is_some() {
            if let Some(actor) = telegram_user_id(&message) {
                match crate::services::student_social::handle_text_flow(
                    &bot, &state.db, &message, actor, "",
                ).await {
                    Ok(true) => return Ok(()),
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!("Dating photo flow failed: {}", error);
                        bot.send_message(
                            message.chat.id,
                            "Не удалось сохранить фото. Попробуй ещё раз через «Фото анкеты».",
                        ).await?;
                        return Ok(());
                    }
                }
            }
        }
        // Telegram keeps file captions separate from message text. Run
        // homework detection on ordinary group captions too, while preserving
        // the dedicated /attach_dz caption command below.
        if let Some(caption) = message.caption() {
            if message.chat.id.0 < 0 && !caption.trim_start().starts_with('/') {
                if let Some(actor) = telegram_user_id(&message) {
                    if ensure_user(&state.db, &message).await.is_ok() {
                        match crate::services::homework_candidates::maybe_create(
                            &state.db,
                            message.chat.id.0,
                            message.id.0,
                            actor,
                            caption,
                        )
                        .await
                        {
                            Ok(Some(candidate)) => {
                                let request = bot
                                    .send_message(
                                        message.chat.id,
                                        crate::services::homework_candidates::candidate_text(&candidate),
                                    )
                                    .reply_markup(crate::services::homework_candidates::review_keyboard(candidate.id));
                                let request = match message.thread_id.clone() {
                                    Some(thread) => request.message_thread_id(thread),
                                    None => request,
                                };
                                request.await?;
                                return Ok(());
                            }
                            Ok(None) => {}
                            Err(error) => tracing::warn!("Could not inspect media caption for homework: {}", error),
                        }
                    }
                }
            }

            // /attach_dz ID captions store only the Telegram file reference.
            let mut words = caption.split_whitespace();
            if words.next().map(|v| v.split('@').next().unwrap_or(v)) == Some("/attach_dz") {
                let Some(actor) = telegram_user_id(&message) else {
                    return Ok(());
                };
                if ensure_user(&state.db, &message).await.is_err() {
                    bot.send_message(
                        message.chat.id,
                        "Не удалось открыть профиль. Повтори попытку позже.",
                    )
                    .await?;
                    return Ok(());
                }
                let id = words
                    .next()
                    .and_then(|v| v.parse::<i64>().ok())
                    .filter(|v| *v > 0);
                let Some(homework_id) = id else {
                    bot.send_message(
                        message.chat.id,
                        "В подписи укажи номер задания: /attach_dz ID",
                    )
                    .await?;
                    return Ok(());
                };
                let file = if let Some(photos) = message.photo() {
                    photos.last().map(|photo| {
                        (
                            photo.file.id.to_string(),
                            photo.file.unique_id.to_string(),
                            None,
                            Some("image/jpeg".to_owned()),
                            Some(i64::from(photo.file.size)),
                        )
                    })
                } else if let Some(document) = message.document() {
                    Some((
                        document.file.id.to_string(),
                        document.file.unique_id.to_string(),
                        document.file_name.clone(),
                        document.mime_type.as_ref().map(|v| v.to_string()),
                        Some(i64::from(document.file.size)),
                    ))
                } else if let Some(video) = message.video() {
                    Some((
                        video.file.id.to_string(),
                        video.file.unique_id.to_string(),
                        video.file_name.clone(),
                        video.mime_type.as_ref().map(|v| v.to_string()),
                        Some(i64::from(video.file.size)),
                    ))
                } else {
                    None
                };
                let answer = if let Some((file_id, unique_id, name, mime, size)) = file {
                    crate::services::group_content::attach_homework(
                        &state.db,
                        message.chat.id.0,
                        actor,
                        homework_id,
                        &file_id,
                        &unique_id,
                        name.as_deref(),
                        mime.as_deref(),
                        size,
                    )
                    .await
                    .unwrap_or_else(|| "Не удалось сохранить вложение.".into())
                } else {
                    "Поддерживаются фото, документы и видео с подписью /attach_dz ID.".into()
                };
                bot.send_message(message.chat.id, answer).await?;
            }
        }
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
        "📚 ДЗ" => if message.chat.id.0 < 0 { "/dz" } else { "/homework" },
        "🗓 Расписание" => "/today",
        "⏰ Дедлайны" => "/deadlines",
        "🏫 Аудитории" => "/free_rooms",
        "👨‍🏫 Преподаватель" => "/teacher",
        "❓ Помощь" => "/help",
        "📅 Сегодня" => "/today",
        "📅 Завтра" => "/tomorrow",
        "🗓 Неделя" => "/week",
        "⏭ Следующая пара" => "/next",
        "📚 Задания" => "/homework",
        "🔔 Напоминания" => "/reminders",
        "⚙️ Настройки" => if message.chat.id.0 < 0 { "/chat_settings" } else { "/settings" },
        "👤 Кабинет" => "/profile",
        "💚 Знакомства" => "/dating",
        "🏠 Меню" => "/menu",
        other => other
            .splitn(2, char::is_whitespace)
            .next()
            .unwrap_or("")
            .split('@')
            .next()
            .unwrap_or(""),
    };

    let argument = message_argument(normalized_text);

    let is_group_chat = message.chat.id.0 < 0;

    match crate::services::student_social::handle_text_flow(
        &bot,
        db,
        &message,
        telegram_id,
        normalized_text,
    )
    .await
    {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(error) => tracing::warn!("Student profile text flow failed: {}", error),
    }

    if let Some(answer) = crate::services::homework_candidates::handle_flow(
        db,
        message.chat.id.0,
        telegram_id,
        normalized_text,
    )
    .await
    {
        reply(&bot, db, &message, answer, &state.config.webapp_url).await?;
        return Ok(());
    }

    // Replies to the bot use opt-in, per-chat Neon context. A reply to another
    // participant is not collected and does not trigger the assistant.
    if is_group_chat && !normalized_text.starts_with('/') {
        match maybe_answer_bot_reply(
            &bot, db, &state.config.webapp_url, &message, telegram_id, normalized_text,
        ).await {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(error) => {
                tracing::warn!("Group reply assistant failed: {}", error);
                return Ok(());
            }
        }
    }

    // Deterministic conversational interface: answer ordinary natural-language
    // requests from the existing schedule and group data, without calling an LLM.
    // In group chats it responds only to explicit bot mentions when the assistant
    // was enabled by an administrator; replies to bot posts are handled above.
    if !normalized_text.starts_with('/') {
        let lower_message = normalized_text.to_lowercase().replace('ё', "е");
        let explicitly_addressed = ["барни", "барней", "spbftu", "spbftuit"]
            .iter()
            .any(|token| lower_message.contains(token));
        let assistant_enabled = if is_group_chat {
            sqlx::query_scalar::<_, bool>(
                "SELECT assistant_enabled FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE"
            ).bind(message.chat.id.0).fetch_optional(db).await.ok().flatten().unwrap_or(false)
        } else {
            true
        };
        let may_answer = !is_group_chat || (assistant_enabled && explicitly_addressed);

        if may_answer {
            if let Some(intent) = crate::services::conversation::detect(normalized_text) {
                let answer = match intent {
                    crate::services::conversation::Intent::Greeting => {
                        Some("Привет! Я Барни, учебный помощник СПбГЛТУ. Могу подсказать расписание, домашние задания, дедлайны, преподавателей и свободные аудитории. Напиши вопрос обычным сообщением или выбери раздел кнопками.".to_owned())
                    }
                    crate::services::conversation::Intent::Help => {
                        Some("Я понимаю обычные вопросы о расписании и учебной группе без нейросети. Например:\n• «Какие пары завтра?»\n• «Что у нас сегодня?»\n• «Какая следующая пара?»\n• «Какие есть домашние задания?»\n• «Когда дедлайны?»\n• «Какие аудитории свободны на 15:00?»\n• «Найди преподавателя Иванова»\n\nДля личного кабинета и знакомств открой личный чат с ботом. Частые действия доступны кнопками меню.".to_owned())
                    }
                    crate::services::conversation::Intent::Today
                    | crate::services::conversation::Intent::Tomorrow
                    | crate::services::conversation::Intent::Week
                    | crate::services::conversation::Intent::NextLesson => {
                        let command = match intent {
                            crate::services::conversation::Intent::Today => "/today",
                            crate::services::conversation::Intent::Tomorrow => "/tomorrow",
                            crate::services::conversation::Intent::Week => "/week",
                            _ => "/next",
                        };
                        Some(match selected_group(db, telegram_id, message.chat.id.0).await {
                            Some(group_id) => schedule_text(db, group_id, command).await,
                            None => "Сначала выбери учебную группу кнопкой «Кабинет» или командой /group название. После этого я смогу отвечать по твоему расписанию.".into(),
                        })
                    }
                    crate::services::conversation::Intent::Homework => {
                        if is_group_chat {
                            crate::services::group_content::handle_homework(
                                db, message.chat.id.0, telegram_id, "/dz", "",
                            ).await
                        } else {
                            match selected_group(db, telegram_id, message.chat.id.0).await {
                                Some(group_id) => Some(
                                    crate::services::group_content::homework_for_group(db, group_id, telegram_id).await
                                ),
                                None => Some("Сначала выбери учебную группу командой /group название. После этого я смогу показать ДЗ, опубликованные в привязанном учебном чате.".into()),
                            }
                        }
                    }
                    crate::services::conversation::Intent::HomeworkFor(subject) => {
                        let group_id = if is_group_chat {
                            sqlx::query_scalar::<_, uuid::Uuid>(
                                "SELECT group_id FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE",
                            )
                            .bind(message.chat.id.0)
                            .fetch_optional(db)
                            .await
                            .ok()
                            .flatten()
                        } else {
                            selected_group(db, telegram_id, message.chat.id.0).await
                        };
                        match group_id {
                            Some(group_id) => Some(
                                crate::services::group_content::homework_for_group_subject(
                                    db, group_id, telegram_id, &subject,
                                ).await
                            ),
                            None => Some("Сначала привяжи учебную группу к этому чату командой /setgroup или выбери её в личных настройках /group.".into()),
                        }
                    }
                    crate::services::conversation::Intent::Deadlines => {
                        crate::services::student_tools::handle(
                            db, message.chat.id.0, telegram_id, "/deadlines", "", None, None,
                        ).await
                    }
                    crate::services::conversation::Intent::FreeRooms(time) => {
                        crate::services::student_tools::handle(
                            db, message.chat.id.0, telegram_id, "/free_rooms",
                            time.as_deref().unwrap_or(""), None, None,
                        ).await
                    }
                    crate::services::conversation::Intent::Teacher(query) => {
                        crate::services::student_tools::handle(
                            db, message.chat.id.0, telegram_id, "/teacher",
                            &query, None, None,
                        ).await
                    }
                };
                let answer = answer.or_else(|| {
                    explicitly_addressed.then(|| {
                        "Я Барни, локальный учебный помощник. Пока понимаю вопросы о расписании, следующей паре, ДЗ, дедлайнах, преподавателях и свободных аудиториях. Сформулируй вопрос по одной из этих тем.".to_owned()
                    })
                });
                if let Some(answer) = answer {
                    reply(&bot, db, &message, answer, &state.config.webapp_url).await?;
                    return Ok(());
                }
            }
        }
    }

    if command == "/start" {
        if is_group_chat {
            reply(&bot, db, &message, "Я помогу с расписанием и заданиями этой группы. Администратор может настроить её командой /setgroup. Личные настройки открой в чате с ботом: /start".into(), &state.config.webapp_url).await?;
        } else if argument == "dating_edit" {
            crate::services::student_social::open_dating_editor(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await?;
        } else if argument == "dating_home" {
            crate::services::student_social::open_dating(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await?;
        } else {
            start_onboarding(&bot, &state, &message).await?;
        }
        return Ok(());
    }

    if command == "/menu" || command == "/help" || command == "/settings" {
        let title = if command == "/help" {
            "Помощь\n\nОсновные разделы доступны кнопками. Выбери группу командой /group, затем открой расписание, задания и дедлайны через меню."
        } else if command == "/settings" && is_group_chat {
            "Настройки группы доступны администраторам через /chat_settings. Личные уведомления открой в личном чате с ботом."
        } else {
            "Главное меню · выбери раздел."
        };
        let settings = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT settings FROM users WHERE telegram_id=$1",
        )
        .bind(telegram_id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        let reminders = settings["reminders_enabled"] == true;
        let reminder_minutes = settings["reminder_minutes"].as_i64().unwrap_or(10);
        let screen = if command == "/help" || (command == "/settings" && is_group_chat) {
            "help"
        } else if command == "/settings" {
            "settings"
        } else {
            "main"
        };
        let text = if command == "/settings" && !is_group_chat {
            format!("Настройки уведомлений\n\nНапоминания о паре: {} · за {} мин\nЕжедневный дайджест: {} · {}\nИзменения расписания: {}",
                if reminders { "вкл" } else { "выкл" }, reminder_minutes,
                if settings["daily_digest_enabled"] == true { "вкл" } else { "выкл" },settings["daily_digest_time"].as_str().unwrap_or("07:00"),
                if settings["schedule_changes_enabled"] == true { "вкл" } else { "выкл" })
        } else {
            title.to_owned()
        };
        bot.send_message(message.chat.id, text)
            .reply_markup(ui_keyboard(screen, reminders, reminder_minutes))
            .await?;
        return Ok(());
    }

    if command == "/commands" {
        bot.send_message(message.chat.id, commands_text(is_group_chat))
            .reply_markup(InlineKeyboardMarkup::new(vec![vec![ui_button(
                "🏠 В меню",
                "ui:main",
            )]]))
            .await?;
        return Ok(());
    }

    if matches!(command, "/profile" | "/dating" | "/dating_reports") {
        if is_group_chat {
            reply(
                &bot,
                db,
                &message,
                "Личный кабинет и знакомства открываются в личном чате: нажми /start у бота."
                    .into(),
                &state.config.webapp_url,
            )
            .await?;
        } else if command == "/profile" {
            crate::services::student_social::open_cabinet(&bot, db, message.chat.id, telegram_id)
                .await?;
        } else if command == "/dating" {
            crate::services::student_social::open_dating(&bot, db, message.chat.id, telegram_id)
                .await?;
        } else {
            crate::services::student_social::open_admin_reports(
                &bot,
                db,
                message.chat.id,
                telegram_id,
                state.config.admin_telegram_id,
            )
            .await?;
        }
        return Ok(());
    }

    if command == "/autodz" || command == "/autodz_queue" {
        if let Err(error) = crate::services::homework_candidates::send_pending_queue(
            &bot,
            db,
            message.chat.id,
            telegram_id,
        )
        .await
        {
            tracing::error!("Could not open homework auto-collection queue: {}", error);
            bot.send_message(message.chat.id, "Не удалось загрузить очередь автосбора ДЗ. Проверь backend log и подключение к базе данных.")
                .await?;
        }
        return Ok(());
    }

    if matches!(
        command,
        "/home"
            | "/teacher"
            | "/teachers"
            | "/free_rooms"
            | "/deadlines"
            | "/resources"
            | "/add_resource"
            | "/report"
            | "/report_queue"
            | "/report_resolve"
            | "/lost_found"
            | "/lost"
            | "/found"
            | "/autodz_on"
            | "/autodz_off"
            | "/autodz_approver"
            | "/subject_alias"
            | "/subject_aliases"
            | "/spam_filter_on"
            | "/spam_filter_off"
    ) {
        let reply_to = message.reply_to_message();
        let target_id = reply_to
            .and_then(|replied| replied.from.as_ref())
            .and_then(|user| i64::try_from(user.id.0).ok());
        let source_message_id = reply_to.map(|replied| replied.id.0);
        if let Some(answer) = crate::services::student_tools::handle(
            db,
            message.chat.id.0,
            telegram_id,
            command,
            argument,
            target_id,
            source_message_id,
        )
        .await
        {
            reply(&bot, db, &message, answer, &state.config.webapp_url).await?;
            return Ok(());
        }
    }

    if command == "/poll" {
        if !is_group_chat {
            reply(
                &bot,
                db,
                &message,
                "Опросы создаются в учебном групповом чате.".into(),
                &state.config.webapp_url,
            )
            .await?;
            return Ok(());
        }
        let can_create: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND owner_telegram_id=$2 AND is_active=TRUE)
               OR EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND telegram_status IN ('creator','administrator'))
               OR EXISTS(SELECT 1 FROM bot_chat_member_roles WHERE chat_id=$1 AND telegram_id=$2 AND role='class_leader')"#
        ).bind(message.chat.id.0).bind(telegram_id).fetch_one(db).await.unwrap_or(false);
        if !can_create {
            reply(
                &bot,
                db,
                &message,
                "Опросы может создать староста или администратор чата.".into(),
                &state.config.webapp_url,
            )
            .await?;
            return Ok(());
        }
        let fields = argument.split('|').map(str::trim).collect::<Vec<_>>();
        if fields.len() < 3
            || fields.len() > 11
            || fields[0].chars().count() > 300
            || fields
                .iter()
                .skip(1)
                .any(|option| option.is_empty() || option.chars().count() > 100)
        {
            reply(
                &bot,
                db,
                &message,
                "Укажи вопрос и от 2 до 10 вариантов через |: /poll Когда встречаемся? | Вт | Ср"
                    .into(),
                &state.config.webapp_url,
            )
            .await?;
            return Ok(());
        }
        let request = bot
            .send_poll(
                message.chat.id,
                fields[0].to_owned(),
                fields[1..].iter().map(|value| value.to_string()),
            )
            .is_anonymous(true)
            .allows_multiple_answers(false);
        let request = match message.thread_id.clone() {
            Some(thread) => request.message_thread_id(thread),
            None => request,
        };
        match request.await {
            Ok(sent) => {
                if let Some(poll) = sent.poll() {
                    let _ = sqlx::query("INSERT INTO bot_group_polls(chat_id,message_id,poll_id,question,created_by) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
                        .bind(message.chat.id.0).bind(sent.id.0).bind(&poll.id).bind(&poll.question).bind(telegram_id).execute(db).await;
                }
            }
            Err(error) => tracing::warn!("Could not create group poll: {}", error),
        }
        return Ok(());
    }

    if is_group_chat && !normalized_text.starts_with('/') {
        // Homework detection gets first refusal. A repeated-message spam
        // signal must never swallow the only copy of a homework announcement.
        match crate::services::homework_candidates::maybe_create(
            db,
            message.chat.id.0,
            message.id.0,
            telegram_id,
            normalized_text,
        )
        .await
        {
            Ok(Some(candidate)) => {
                let request = bot
                    .send_message(
                        message.chat.id,
                        crate::services::homework_candidates::candidate_text(&candidate),
                    )
                    .reply_markup(crate::services::homework_candidates::review_keyboard(
                        candidate.id,
                    ));
                let request = match message.thread_id.clone() {
                    Some(thread) => request.message_thread_id(thread),
                    None => request,
                };
                request.await?;
                return Ok(());
            }
            Ok(None) => {}
            Err(error) => tracing::warn!("Could not inspect group message for homework: {}", error),
        }
        if crate::services::student_tools::detect_repeated_spam(
            db,
            message.chat.id.0,
            telegram_id,
            message.id.0,
            normalized_text,
        )
        .await
        {
            return Ok(());
        }
    }

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

    if is_group_chat
        && matches!(
            command,
            "/dz"
                | "/homework"
                | "/add_dz"
                | "/edit_dz"
                | "/delete_dz"
                | "/done"
                | "/my_dz"
                | "/priority_dz"
        )
    {
        if matches!(
            command,
            "/add_dz" | "/edit_dz" | "/delete_dz" | "/priority_dz"
        ) {
            let _ = crate::services::chat_roles::sync_member(
                &bot,
                db,
                message.chat.id,
                telegram_id,
                None,
            )
            .await;
        }
        if command == "/delete_dz" {
            let Ok(id) = argument.parse::<i64>() else {
                reply(
                    &bot,
                    db,
                    &message,
                    "Формат: /delete_dz ID".into(),
                    &state.config.webapp_url,
                )
                .await?;
                return Ok(());
            };
            if !crate::services::group_content::can_edit_homework(
                db,
                message.chat.id.0,
                telegram_id,
            )
            .await
            {
                reply(
                    &bot,
                    db,
                    &message,
                    "⛔ Удалять задания может только пользователь с правом редактирования ДЗ."
                        .into(),
                    &state.config.webapp_url,
                )
                .await?;
                return Ok(());
            }
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bot_homework WHERE chat_id=$1 AND id=$2 AND status='active')")
                .bind(message.chat.id.0).bind(id).fetch_one(db).await.unwrap_or(false);
            if !exists {
                reply(
                    &bot,
                    db,
                    &message,
                    "Активное задание с таким номером не найдено.".into(),
                    &state.config.webapp_url,
                )
                .await?;
                return Ok(());
            }
            let keyboard = InlineKeyboardMarkup::new(vec![vec![
                InlineKeyboardButton::callback("🗄 Удалить", format!("dzdel:{id}:yes")),
                InlineKeyboardButton::callback("Отмена", format!("dzdel:{id}:no")),
            ]]);
            let request = bot
                .send_message(
                    message.chat.id,
                    format!(
                        "Подтверди удаление задания #{id}. Оно будет убрано из активного списка."
                    ),
                )
                .reply_markup(keyboard);
            let request = match message.thread_id.clone() {
                Some(thread) => request.message_thread_id(thread),
                None => request,
            };
            request.await?;
            return Ok(());
        }
        if let Some(answer) = crate::services::group_content::handle_homework(
            db,
            message.chat.id.0,
            telegram_id,
            command,
            argument,
        )
        .await
        {
            reply(&bot, db, &message, answer, &state.config.webapp_url).await?;
            return Ok(());
        }
    }

    if is_group_chat && matches!(command, "/search" | "/group_stats" | "/my_stats") {
        if let Some(answer) = crate::services::group_content::handle_group_tools(
            db,
            message.chat.id.0,
            telegram_id,
            command,
            argument,
        )
        .await
        {
            reply(&bot, db, &message, answer, &state.config.webapp_url).await?;
            return Ok(());
        }
    }

    if is_group_chat && matches!(command, "/content_rights" | "/content_editor") {
        let _ =
            crate::services::chat_roles::sync_member(&bot, db, message.chat.id, telegram_id, None)
                .await;
        if let Some(answer) = crate::services::group_content::handle_content_admin(
            &bot,
            db,
            message.chat.id.0,
            telegram_id,
            command,
            argument,
        )
        .await
        {
            reply(&bot, db, &message, answer, &state.config.webapp_url).await?;
            return Ok(());
        }
    }

    if is_group_chat && matches!(command, "/owner" | "/transfer_owner") {
        let _ =
            crate::services::chat_roles::sync_member(&bot, db, message.chat.id, telegram_id, None)
                .await;
        if let Some(answer) = crate::services::chat_roles::handle_ownership(
            &bot,
            db,
            message.chat.id,
            telegram_id,
            command,
            argument,
        )
        .await
        {
            reply(&bot, db, &message, answer, &state.config.webapp_url).await?;
            return Ok(());
        }
    }

    if command == "/export_calendar" {
        if !is_group_chat {
            reply(
                &bot,
                db,
                &message,
                "Экспорт календаря доступен в настроенном учебном чате: /export_calendar".into(),
                &state.config.webapp_url,
            )
            .await?;
            return Ok(());
        }
        match crate::services::calendar_export::build_group_calendar(db, message.chat.id.0).await {
            Ok(Some(calendar)) => {
                let file =
                    InputFile::memory(calendar.into_bytes()).file_name("SPbFTU-SMART-CAMPUS.ics");
                let request = bot
                    .send_document(message.chat.id, file)
                    .caption("📅 Календарь SPbFTU SMART CAMPUS: расписание, экзамены и мероприятия на ближайшие 180 дней.");
                let request = match message.thread_id.clone() {
                    Some(thread_id) => request.message_thread_id(thread_id),
                    None => request,
                };
                request.await?;
            }
            Ok(None) => {
                reply(&bot, db, &message, "Для этой группы нет будущих занятий, экзаменов или мероприятий либо учебная группа ещё не настроена.".into(), &state.config.webapp_url).await?;
            }
            Err(error) => {
                tracing::error!(
                    "Calendar export failed for chat {}: {}",
                    message.chat.id.0,
                    error
                );
                reply(&bot, db, &message, "Не удалось сформировать календарь. Проверьте настройки учебного чата и попробуйте позже.".into(), &state.config.webapp_url).await?;
            }
        }
        return Ok(());
    }

    if is_group_chat
        && matches!(
            command,
            "/exams"
                | "/add_exam"
                | "/edit_exam"
                | "/exam_info"
                | "/delete_exam"
                | "/events"
                | "/add_event"
                | "/edit_event"
                | "/cancel_event"
                | "/join_event"
                | "/leave_event"
                | "/event_capacity"
                | "/event_waitlist"
                | "/event_attendees"
                | "/event_maybe"
                | "/event_no"
                | "/event_reminders_on"
                | "/event_reminders_off"
        )
    {
        if matches!(
            command,
            "/add_exam"
                | "/edit_exam"
                | "/exam_info"
                | "/delete_exam"
                | "/add_event"
                | "/edit_event"
                | "/cancel_event"
                | "/event_capacity"
                | "/event_waitlist"
        ) {
            let _ = crate::services::chat_roles::sync_member(
                &bot,
                db,
                message.chat.id,
                telegram_id,
                None,
            )
            .await;
        }
        if let Some(answer) = crate::services::group_content::handle_exams_events(
            &bot,
            db,
            message.chat.id.0,
            telegram_id,
            command,
            argument,
        )
        .await
        {
            reply(&bot, db, &message, answer, &state.config.webapp_url).await?;
            return Ok(());
        }
    }

    if command == "/group" && !is_group_chat && argument.is_empty() {
        crate::services::student_social::open_group_picker(
            &bot,
            db,
            message.chat.id,
        )
        .await?;
        return Ok(());
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
            👮 /roles — список ролей и полномочий\n\
            👮 /role — назначить роль участнику\n\
            📚 /homework — домашние задания\n\
            🔔 /remind_on — включить личные напоминания\n\
            🔕 /remind_off — выключить напоминания\n\
            ⚙️ /settings — настройки\n\n\
            Выбери действие с помощью кнопок ниже."
                .to_owned();
            if is_group_chat {
                help.push_str(
                    "\n\n🔎 /search запрос — поиск по учебным материалам\n📊 /group_stats — статистика учебного чата\n📈 /my_stats — мой прогресс по ДЗ\n📚 *Учебные материалы чата*\n/dz — домашние задания\n/add_dz Предмет | задание | срок — добавить ДЗ\n/edit_dz ID | предмет | задание | срок — изменить ДЗ\n/done ID — отметить ДЗ выполненным для себя\n/my_dz — мои невыполненные задания\n/priority_dz ID urgent|normal — приоритет\n/content_rights — права на разделы\n/content_editor — назначить редактора\n\n📝 *Экзамены и зачёты*\n/exams — список экзаменов\n/add_exam Предмет | дата время | аудитория | формат | темы | литература | описание\n/edit_exam ID | предмет | дата время | аудитория | формат | темы | литература | описание\n/exam_info ID | формат | темы | литература | описание\n/delete_exam ID — архивировать\n\n🎉 *Мероприятия*\n/events — список мероприятий\n/add_event Название | дата время | место | описание\n/join_event ID и /leave_event ID — запись и отмена\n/event_maybe ID и /event_no ID — изменить ответ\n/event_attendees ID — список участников\n/event_capacity ID число|off — лимит мест\n/event_waitlist ID on|off — лист ожидания\n/event_reminders_on ID и /event_reminders_off ID — личные напоминания\n/edit_event ID | название | дата время | место | описание\n/cancel_event ID — отменить мероприятие\n/export_calendar — получить календарь .ics\n/owner и /transfer_owner @username — владелец SMART CAMPUS\n\n📢 *Автоматизация учебного чата*\n/chat_settings — настройки\n/daily_on — расписание каждый день\n/daily_time 07:30 — время публикации (МСК)\n/chat_notify_on — уведомлять за 10 минут до пары\n/daily_off и /chat_notify_off — выключить функции\n/chat_topic — выбрать тему (отправить внутри темы)\n/chat_topic_off — публиковать в общий чат\n\n🛡 *Модераторам*\n/roles — роли и права\nОтветом: /warn причина · /mute 30m причина · /unmute · /ban\nПо ID: /warnings ID · /unban ID",
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
                "⛔ Менять учебную группу чата может только владелец SMART CAMPUS, создатель или администратор Telegram."
                    .into()
            } else if argument.is_empty() {
                let current = sqlx::query_scalar::<_, String>(
                    "SELECT g.name FROM bot_chat_settings c JOIN groups g ON g.id=c.group_id WHERE c.chat_id=$1"
                ).bind(message.chat.id.0).fetch_optional(db).await.ok().flatten();
                let text = format!("🏫 Настройка учебной группы чата\\nТекущая группа: {}\\nВыбери новую группу ниже.",current.unwrap_or_else(||"не выбрана".into()));
                let keyboard = InlineKeyboardMarkup::new(vec![vec![ui_button("📚 Выбрать группу","chatgroup:open")],vec![ui_button("🏠 Меню","ui:main")]]);
                bot.send_message(message.chat.id,text).reply_markup(keyboard).await?;
                return Ok(());
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

        "/roles" => {
            if !is_group_chat {
                "Роли модерации настраиваются в групповом чате.".into()
            } else {
                "🛡 РОЛИ И ПРАВА\n\n👑 Администратор Telegram — управление чатом и назначение ролей.\n⭐ class_leader — подтверждение ДЗ из чата, материалы и опросы.\n🛡 senior_moderator / moderator / helper — уровни модерации.\n👤 student — базовая роль; участник может также иметь дополнительные роли.\n\nНазначение ответом на сообщение: /role class_leader\nСнять роль старосты: /role student\nМодерация: /role moderator или /role helper".into()
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
                                "moderator" | "helper" | "member" | "class_leader" | "student"
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

                                            if requested_role == "class_leader" {
                                                sqlx::query("INSERT INTO bot_chat_member_roles(chat_id,telegram_id,role,assigned_by) VALUES($1,$2,'class_leader',$3) ON CONFLICT(chat_id,telegram_id,role) DO NOTHING")
                                                    .bind(message.chat.id.0).bind(target_id).bind(telegram_id).execute(&mut *tx).await?;
                                            } else if requested_role == "student" {
                                                sqlx::query("DELETE FROM bot_chat_member_roles WHERE chat_id=$1 AND telegram_id=$2 AND role='class_leader'")
                                                    .bind(message.chat.id.0).bind(target_id).execute(&mut *tx).await?;
                                                sqlx::query("UPDATE bot_chat_members SET role='member',assigned_by=$3,updated_at=now() WHERE chat_id=$1 AND telegram_id=$2")
                                                    .bind(message.chat.id.0).bind(target_id).bind(telegram_id).execute(&mut *tx).await?;
                                            } else {
                                                sqlx::query("UPDATE bot_chat_members SET role=$3,assigned_by=$4,updated_at=now() WHERE chat_id=$1 AND telegram_id=$2")
                                                    .bind(message.chat.id.0).bind(target_id).bind(&requested_role).bind(telegram_id).execute(&mut *tx).await?;
                                            }

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
                                            set_member_command_menu(&bot, message.chat.id, target_id, &requested_role).await;
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
            } else if !crate::services::chat_roles::can_warn_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Предупреждения могут выдавать модераторы чата.".into()
            } else {
                match parse_target_and_tail(db, message.chat.id.0, &message, argument).await {
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
            } else if !crate::services::chat_roles::can_warn_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Смотреть предупреждения могут модераторы чата.".into()
            } else {
                match parse_target_and_tail(db, message.chat.id.0, &message, argument).await {
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
            } else if !crate::services::chat_roles::can_mute_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Ограничивать участников могут модераторы чата.".into()
            } else {
                match parse_target_and_tail(db, message.chat.id.0, &message, argument).await {
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
            } else if !crate::services::chat_roles::can_mute_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Снимать ограничения могут модераторы чата.".into()
            } else {
                match parse_target_and_tail(db, message.chat.id.0, &message, argument).await {
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
            } else if !crate::services::chat_roles::can_ban_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Блокировать участников могут модераторы чата.".into()
            } else {
                match parse_target_and_tail(db, message.chat.id.0, &message, argument).await {
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
                                let reason = reason.trim().to_owned();
                                let saved = sqlx::query("INSERT INTO bot_user_flows(chat_id,telegram_id,flow,step,payload,expires_at) VALUES($1,$2,'ban_confirm','confirm',jsonb_build_object('target_id',$3,'reason',$4),now()+interval '5 minutes') ON CONFLICT(chat_id,telegram_id) DO UPDATE SET flow=EXCLUDED.flow,step=EXCLUDED.step,payload=EXCLUDED.payload,expires_at=EXCLUDED.expires_at,updated_at=now()")
                                    .bind(message.chat.id.0).bind(telegram_id).bind(target_id).bind(&reason).execute(db).await;
                                if saved.is_err() {
                                    "Не удалось подготовить подтверждение; блокировка не применена."
                                        .into()
                                } else {
                                    let keyboard = InlineKeyboardMarkup::new(vec![vec![
                                        InlineKeyboardButton::callback(
                                            "⛔ Подтвердить бан",
                                            "ban:yes",
                                        ),
                                        InlineKeyboardButton::callback("Отмена", "ban:no"),
                                    ]]);
                                    let prompt = if reason.is_empty() {
                                        format!("Заблокировать участника {target_id}?")
                                    } else {
                                        format!("Заблокировать участника {target_id}? Причина: {reason}")
                                    };
                                    let request = bot
                                        .send_message(message.chat.id, prompt)
                                        .reply_markup(keyboard);
                                    let request = match message.thread_id.clone() {
                                        Some(thread) => request.message_thread_id(thread),
                                        None => request,
                                    };
                                    request.await?;
                                    return Ok(());
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
            } else if !crate::services::chat_roles::can_ban_chat(
                &bot,
                db,
                message.chat.id,
                telegram_id,
            )
            .await
            {
                "⛔ Разблокировать участников могут модераторы чата.".into()
            } else {
                match parse_target_and_tail(db, message.chat.id.0, &message, argument).await {
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
                "⛔ Выбирать тему публикаций может только создатель или администратор Telegram."
                    .into()
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
                "⛔ Изменять тему публикаций может только создатель или администратор Telegram."
                    .into()
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
                "⛔ Настраивать расписание и уведомления может только создатель или администратор Telegram.".into()
            } else {
                let (column, enabled, label) = match command {
                    "/daily_on" => (
                        "daily_schedule_enabled",
                        true,
                        "ежедневная публикация расписания",
                    ),
                    "/daily_off" => (
                        "daily_schedule_enabled",
                        false,
                        "ежедневная публикация расписания",
                    ),
                    "/chat_notify_on" => (
                        "lesson_notifications_enabled",
                        true,
                        "уведомления о начале пар",
                    ),
                    _ => (
                        "lesson_notifications_enabled",
                        false,
                        "уведомления о начале пар",
                    ),
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
                        "{} {}.\nТекущие параметры: /chat_settings",
                        if enabled {
                            "✅ Включена"
                        } else {
                            "🔕 Выключена"
                        },
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
        "/chat_timezone" => {
            if !is_group_chat {
                "Часовой пояс настраивается только в учебном групповом чате.".into()
            } else if argument.trim().is_empty() {
                let current: Option<String> = sqlx::query_scalar(
                    "SELECT timezone FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE",
                )
                .bind(message.chat.id.0)
                .fetch_optional(db)
                .await
                .ok()
                .flatten();
                current.map(|zone| format!("🌍 Часовой пояс учебного чата: {zone}\nИзменить: /chat_timezone Europe/Helsinki"))
                    .unwrap_or_else(|| "Сначала привяжите учебную группу: /setgroup название".into())
            } else {
                let app_owner: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND owner_telegram_id=$2 AND is_active=TRUE)"
                ).bind(message.chat.id.0).bind(telegram_id).fetch_one(db).await.unwrap_or(false);
                let role = crate::services::chat_roles::sync_member(
                    &bot,
                    db,
                    message.chat.id,
                    telegram_id,
                    None,
                )
                .await
                .ok()
                .flatten();
                if !app_owner && !matches!(role.as_deref(), Some("owner" | "admin")) {
                    "⛔ Изменять часовой пояс может только владелец SMART CAMPUS или администратор Telegram.".into()
                } else {
                    let timezone = argument.trim();
                    let valid: bool = sqlx::query_scalar(
                        "SELECT EXISTS(SELECT 1 FROM pg_timezone_names WHERE name=$1)",
                    )
                    .bind(timezone)
                    .fetch_one(db)
                    .await
                    .unwrap_or(false);
                    if !valid {
                        "Неизвестный часовой пояс. Укажи IANA-идентификатор, например Europe/Moscow, Europe/Helsinki или Asia/Tokyo.".into()
                    } else {
                        match sqlx::query(
                            "UPDATE bot_chat_settings SET timezone=$2,updated_at=now() WHERE chat_id=$1 AND is_active=TRUE"
                        ).bind(message.chat.id.0).bind(timezone).execute(db).await {
                            Ok(result) if result.rows_affected()==1 => format!(
                                "✅ Часовой пояс учебного чата установлен: {timezone}.\nДаты ДЗ, экзаменов, мероприятий и ежедневные публикации теперь используют это время."
                            ),
                            Ok(_) => "Сначала привяжите учебную группу: /setgroup название".into(),
                            Err(error) => {
                                tracing::error!("Could not update group timezone: {}", error);
                                "Не удалось сохранить часовой пояс.".into()
                            }
                        }
                    }
                }
            }
        }

        "/daily_time" => {
            if !is_group_chat {
                "Время публикации настраивается только в групповом чате.".into()
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
                "⛔ Менять время публикации может только создатель или администратор Telegram."
                    .into()
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
                            "🕒 Время ежедневной публикации установлено: {} (местное время учебного чата).",
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
            } else if !matches!(
                crate::services::chat_roles::sync_member(&bot, db, message.chat.id, telegram_id, None).await.ok().flatten().as_deref(),
                Some("owner" | "admin")
            ) {
                "⛔ Открывать настройки учебного чата могут только администраторы Telegram.".into()
            } else {
                let row=sqlx::query_as::<_,(String,bool,bool,bool,bool,bool,bool,String)>(
                    "SELECT g.name,c.daily_schedule_enabled,c.lesson_notifications_enabled,c.daily_digest_enabled,c.assistant_enabled,c.context_enabled,c.homework_autocollect_enabled,c.homework_approver_role FROM bot_chat_settings c JOIN groups g ON g.id=c.group_id WHERE c.chat_id=$1 AND c.is_active=TRUE"
                ).bind(message.chat.id.0).fetch_optional(db).await;
                match row {
                    Ok(Some((group,daily,lessons,digest,assistant,context,autodz,approver))) => {
                        let keyboard=chat_settings_keyboard(message.chat.id,&group,daily,lessons,digest,assistant,context,autodz,&approver);
                        let text=format!("⚙️ НАСТРОЙКИ УЧЕБНОГО ЧАТА\\n\\nГруппа: {group}\\nЧасовой пояс: Москва (UTC+3)\\nЕжедневная публикация: {}\\nУведомления о парах: {}\\nДайджест: {} · 07:00 МСК\\nПомощник в ответах: {}\\nКонтекст ответов: {}\nАвтосбор ДЗ: {}\nПодтверждают ДЗ: {}",
                            if daily{"вкл"}else{"выкл"},if lessons{"вкл"}else{"выкл"},if digest{"вкл"}else{"выкл"},if assistant{"вкл"}else{"выкл"},if context{"вкл"}else{"выкл"},if autodz{"вкл"}else{"выкл"},match approver.as_str(){"class_leader"=>"староста","moderator"=>"модераторы",_=>"администраторы"});
                        bot.send_message(message.chat.id,text).reply_markup(keyboard).await?;
                        return Ok(());
                    }
                    Ok(None) => "Сначала привяжи учебную группу командой /setgroup или кнопкой ниже.".into(),
                    Err(error) => {
                        tracing::error!("Could not read group settings: {}",error);
                        "Не удалось загрузить настройки учебного чата.".into()
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
                    crate::services::group_content::homework_for_group(db, group_id, telegram_id).await
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
    let global_commands = vec![
        BotCommand::new("start", "Краткое знакомство и меню"),
        BotCommand::new("help", "Помощь и подсказки"),
        BotCommand::new("settings", "Мои настройки"),
        BotCommand::new("profile", "Личный кабинет"),
        BotCommand::new("dating", "Добровольные знакомства 18+"),
        BotCommand::new("dating_reports", "Жалобы на анкеты (администратор)"),
        BotCommand::new("menu", "Главное меню"),
        BotCommand::new("commands", "Показать список команд"),
    ];
    if let Err(error) = bot
        .set_my_commands(global_commands.clone())
        .scope(BotCommandScope::Default)
        .language_code("ru")
        .await
    {
        tracing::warn!("Could not register default Telegram commands: {}", error);
    }
    let private_commands = vec![
        BotCommand::new("start", "Открыть меню"),
        BotCommand::new("help", "Список команд"),
        BotCommand::new("commands", "Показать список команд"),
        BotCommand::new("menu", "Главное меню"),
        BotCommand::new("today", "Расписание на сегодня"),
        BotCommand::new("tomorrow", "Расписание на завтра"),
        BotCommand::new("week", "Расписание на 7 дней"),
        BotCommand::new("next", "Следующая пара"),
        BotCommand::new("group", "Выбрать личную учебную группу"),
        BotCommand::new("homework", "Домашние задания"),
        BotCommand::new("deadlines", "Ближайшие сроки"),
        BotCommand::new("home", "Что дальше"),
        BotCommand::new("teacher", "Найти занятия преподавателя"),
        BotCommand::new("free_rooms", "Свободные аудитории"),
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
        BotCommand::new("commands", "Показать список команд"),
        BotCommand::new("today", "Расписание на сегодня"),
        BotCommand::new("tomorrow", "Расписание на завтра"),
        BotCommand::new("week", "Расписание на 7 дней"),
        BotCommand::new("next", "Следующая пара"),
        BotCommand::new("search", "Поиск по материалам чата"),
        BotCommand::new("resources", "Материалы учебного чата"),
        BotCommand::new("add_resource", "Добавить ссылку в материалы"),
        BotCommand::new("report", "Пожаловаться на сообщение (ответом)"),
        BotCommand::new("report_queue", "Открытые жалобы"),
        BotCommand::new("report_resolve", "Закрыть жалобу по номеру"),
        BotCommand::new("autodz", "Очередь собранных заданий на проверку"),
        BotCommand::new("autodz_on", "Включить сбор кандидатов ДЗ"),
        BotCommand::new("autodz_off", "Выключить сбор кандидатов ДЗ"),
        BotCommand::new("autodz_approver", "Выбрать роль подтверждения ДЗ"),
        BotCommand::new("subject_alias", "Добавить сокращение предмета"),
        BotCommand::new("subject_aliases", "Список сокращений предметов"),
        BotCommand::new("spam_filter_on", "Включить фильтр повторов"),
        BotCommand::new("spam_filter_off", "Выключить фильтр повторов"),
        BotCommand::new("poll", "Создать опрос группы"),
        BotCommand::new("lost_found", "Потерянные и найденные вещи"),
        BotCommand::new("lost", "Сообщить о потерянной вещи"),
        BotCommand::new("found", "Сообщить о найденной вещи"),
        BotCommand::new("teacher", "Найти занятия преподавателя"),
        BotCommand::new("free_rooms", "Свободные аудитории"),
        BotCommand::new("group_stats", "Статистика учебного чата"),
        BotCommand::new("my_stats", "Моя статистика по ДЗ"),
        BotCommand::new("homework", "Домашние задания"),
        BotCommand::new("dz", "Список домашних заданий чата"),
        BotCommand::new("add_dz", "Добавить ДЗ: предмет | задание | срок"),
        BotCommand::new("attach_dz", "Прикрепить файл к ДЗ (подпись к файлу)"),
        BotCommand::new("edit_dz", "Изменить ДЗ по ID"),
        BotCommand::new("delete_dz", "Убрать ДЗ по ID"),
        BotCommand::new("done", "Отметить ДЗ выполненным для себя"),
        BotCommand::new("my_dz", "Мои невыполненные задания"),
        BotCommand::new("priority_dz", "Изменить приоритет ДЗ"),
        BotCommand::new("content_rights", "Настроить права разделов"),
        BotCommand::new("content_editor", "Назначить редактора раздела"),
        BotCommand::new("exams", "Список экзаменов и зачётов"),
        BotCommand::new("add_exam", "Добавить экзамен"),
        BotCommand::new("edit_exam", "Изменить экзамен"),
        BotCommand::new("exam_info", "Темы, формат и литература экзамена"),
        BotCommand::new("delete_exam", "Архивировать экзамен"),
        BotCommand::new("export_calendar", "Экспорт расписания в календарь .ics"),
        BotCommand::new("events", "Список мероприятий"),
        BotCommand::new("add_event", "Создать мероприятие"),
        BotCommand::new("edit_event", "Изменить мероприятие"),
        BotCommand::new("cancel_event", "Отменить мероприятие"),
        BotCommand::new("join_event", "Записаться на мероприятие"),
        BotCommand::new("leave_event", "Отменить запись на мероприятие"),
        BotCommand::new("event_capacity", "Установить лимит участников мероприятия"),
        BotCommand::new("event_waitlist", "Включить или выключить лист ожидания"),
        BotCommand::new(
            "event_attendees",
            "Посмотреть ответы участников мероприятия",
        ),
        BotCommand::new("event_maybe", "Возможно, пойду на мероприятие"),
        BotCommand::new("event_no", "Отметить, что не иду на мероприятие"),
        BotCommand::new(
            "event_reminders_on",
            "Включить личные напоминания о мероприятии",
        ),
        BotCommand::new(
            "event_reminders_off",
            "Выключить личные напоминания о мероприятии",
        ),
        BotCommand::new("owner", "Посмотреть владельца SMART CAMPUS"),
        BotCommand::new("transfer_owner", "Передать владение SMART CAMPUS участнику"),
        BotCommand::new("settings", "Настройки учебного чата"),
        BotCommand::new("chat_settings", "Настройки расписания учебного чата"),
        BotCommand::new("chat_timezone", "Установить часовой пояс учебного чата"),
        BotCommand::new("roles", "Список ролей и прав модерации"),
        BotCommand::new("daily_on", "Ежедневно публиковать расписание в чате"),
        BotCommand::new("daily_off", "Отключить ежедневное расписание в чате"),
        BotCommand::new("daily_time", "Установить местное время публикации ЧЧ:ММ"),
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
    admin_commands.push(BotCommand::new(
        "chat_topic",
        "Выбрать текущую тему для публикаций",
    ));
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

/// Starts Telegram in webhook mode for scale-to-zero hosts such as Render Free.
/// Incoming Telegram updates wake the service; no long-polling connection is required.
pub async fn start_webhook_mode(state: Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    let token = state.config.teloxide_token.clone()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "TELOXIDE_TOKEN is not configured"))?;
    let secret = state.config.telegram_webhook_secret.clone()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "TELEGRAM_WEBHOOK_SECRET is required in webhook mode"))?;
    let base_url = std::env::var("TELEGRAM_WEBHOOK_URL")
        .or_else(|_| std::env::var("RENDER_EXTERNAL_URL"))?;
    let webhook_url = format!("{}/telegram/webhook", base_url.trim_end_matches('/'));
    let webhook = url::Url::parse(&webhook_url)?;
    let bot = Bot::new(token);

    bot.set_webhook(webhook)
        .secret_token(secret)
        .drop_pending_updates(false)
        .await?;

    register_bot_commands(&bot).await;
    tracing::info!("Telegram webhook configured: {}", webhook_url);

    let reminder_bot = bot.clone();
    tokio::spawn(async move {
        reminder_loop(reminder_bot, state).await;
    });
    Ok(())
}

/// Dispatches a Telegram webhook update through the same handlers as long polling.
pub async fn handle_webhook_update(state: Arc<AppState>, update: Update) -> ResponseResult<()> {
    let Some(token) = state.config.teloxide_token.clone() else {
        return Ok(());
    };
    let bot = Bot::new(token);
    match update.kind {
        UpdateKind::Message(message) => handle(bot, state, message).await,
        UpdateKind::InlineQuery(query) => handle_inline_query(bot, state, query).await,
        UpdateKind::CallbackQuery(query) => handle_ui_callback(bot, state, query).await,
        _ => Ok(()),
    }
}

pub async fn run(state: Arc<AppState>) {
    let Some(token) = state.config.teloxide_token.clone() else {
        tracing::warn!("TELOXIDE_TOKEN is not configured; bot is disabled");
        return;
    };

    // Only one process may poll updates for this bot token. Without this lock,
    // a local copy and a hosted copy can repeatedly interrupt each other's
    // getUpdates calls, so callback queries never reach a stable dispatcher.
    //
    // Use the direct PostgreSQL endpoint for a session-level advisory lock:
    // PgBouncer transaction pooling does not preserve session locks.
    let lock_url = std::env::var("DATABASE_URL_UNPOOLED")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| state.config.database_url.clone());
    let mut lock_url = match url::Url::parse(&lock_url) {
        Ok(url) => url,
        Err(error) => {
            tracing::error!("Could not parse database URL for Telegram polling lock: {}", error);
            return;
        }
    };
    if let Some(host) = lock_url.host_str().map(str::to_owned) {
        if host.contains("-pooler.") {
            let direct_host = host.replace("-pooler.", ".");
            if let Err(error) = lock_url.set_host(Some(&direct_host)) {
                tracing::error!("Could not select direct database host for Telegram polling lock: {}", error);
                return;
            }
        }
    }

    let lock_pool = match PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .connect(lock_url.as_str())
        .await
    {
        Ok(pool) => pool,
        Err(error) => {
            tracing::error!("Could not connect to the direct database endpoint for Telegram polling lock: {}", error);
            return;
        }
    };
    let mut poll_lock = match lock_pool.acquire().await {
        Ok(connection) => connection,
        Err(error) => {
            tracing::error!("Could not acquire Telegram polling lock connection: {}", error);
            return;
        }
    };
    let lock_acquired = match sqlx::query_scalar::<_, bool>(
        "SELECT pg_try_advisory_lock($1, $2)",
    )
    .bind(188_i32)
    .bind(1_i32)
    .fetch_one(&mut *poll_lock)
    .await
    {
        Ok(acquired) => acquired,
        Err(error) => {
            tracing::error!("Could not acquire Telegram polling advisory lock: {}", error);
            return;
        }
    };
    if !lock_acquired {
        tracing::warn!(
            "Telegram polling is already active in another process; this instance will not start a second dispatcher"
        );
        return;
    }
    tracing::info!("Telegram polling lock acquired; this process is the active bot poller");

    // Keep both the pool and its dedicated connection alive for the lifetime
    // of the dispatcher. Dropping the connection releases the advisory lock.
    let _poll_lock_pool = lock_pool;
    let _poll_lock_connection = poll_lock;

    let bot = Bot::new(token);
    if let Err(error) = bot.get_me().await {
        tracing::error!("Telegram bot token validation failed: {}", error);
        return;
    }
    tracing::info!("Telegram bot token validated; starting long polling");

    // This dispatcher uses getUpdates (long polling). Remove a webhook left by
    // an earlier deployment, but preserve pending updates in Telegram.
    loop {
        match bot.delete_webhook().await {
            Ok(_) => {
                tracing::info!("Telegram webhook cleared; starting long polling");
                break;
            }
            Err(error) => {
                tracing::error!(
                    "Could not remove the previous Telegram webhook; retrying in 10 seconds: {}",
                    error
                );
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    }
    register_bot_commands(&bot).await;

    let reminder_bot = bot.clone();
    let reminder_state = state.clone();

    tokio::spawn(async move {
        reminder_loop(reminder_bot, reminder_state).await;
    });

    let message_state = state.clone();
    let inline_state = state.clone();
    let callback_state = state.clone();

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
        )
        .branch(
            Update::filter_callback_query().endpoint(move |bot: Bot, query: CallbackQuery| {
                let state = callback_state.clone();
                async move {
                    tracing::info!(
                        callback_id = ?query.id,
                        callback_data = ?query.data,
                        "Telegram inline callback received"
                    );
                    match handle_ui_callback(bot, state, query).await {
                        Ok(()) => {
                            tracing::debug!("Telegram inline callback handled successfully");
                            Ok(())
                        }
                        Err(error) => {
                            tracing::error!("Telegram inline callback handler failed: {}", error);
                            Err(error)
                        }
                    }
                }
            }),
        );

    Dispatcher::builder(bot, handler).build().dispatch().await;
}

/// Delivers opt-in daily schedule posts and 10-minute lesson reminders to configured group chats.
/// Delivery rows are reserved before sending to prevent duplicate messages across worker ticks.
async fn record_notification_delivery(
    db: &PgPool,
    kind: &str,
    recipient_kind: &str,
    recipient_id: i64,
    success: bool,
    detail: Option<&str>,
) {
    let status = if success { "sent" } else { "failed" };
    let detail = detail.map(|value| value.chars().take(500).collect::<String>());
    let worker_error = detail.clone();
    if let Err(error) = sqlx::query(
        "INSERT INTO bot_notification_delivery_log(kind,recipient_kind,recipient_id,status,detail) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(kind)
    .bind(recipient_kind)
    .bind(recipient_id)
    .bind(status)
    .bind(detail)
    .execute(db)
    .await
    {
        tracing::warn!("Could not record notification delivery event: {}", error);
    }

    if success {
        let _ = sqlx::query(
            "INSERT INTO bot_notification_worker_state(worker,last_success_at,last_error,updated_at) VALUES('telegram_reminders',now(),NULL,now()) ON CONFLICT(worker) DO UPDATE SET last_success_at=now(),last_error=NULL,updated_at=now()",
        )
        .execute(db)
        .await;
    } else {
        let _ = sqlx::query(
            "INSERT INTO bot_notification_worker_state(worker,last_error,updated_at) VALUES('telegram_reminders',$1,now()) ON CONFLICT(worker) DO UPDATE SET last_error=$1,updated_at=now()",
        )
        .bind(worker_error)
        .execute(db)
        .await;
    }
}

/// Daily schedule and digest are separate posts with independent times and delivery keys.
async fn group_chat_schedule_loop_tick(bot: &Bot, state: &Arc<AppState>) {
    let daily_chats = sqlx::query_as::<_, (i64, uuid::Uuid, NaiveDate, Option<i64>, String)>(
        r#"
        SELECT c.chat_id, c.group_id, (now() AT TIME ZONE 'Europe/Moscow')::date,
               c.message_thread_id, post.post_kind
        FROM bot_chat_settings c
        CROSS JOIN LATERAL (
          VALUES
            ('schedule'::text, c.daily_schedule_enabled, c.daily_schedule_time),
            ('digest'::text, c.daily_digest_enabled, c.daily_digest_time)
        ) AS post(post_kind, enabled, post_time)
        WHERE c.is_active=TRUE
          AND post.enabled=TRUE
          AND post.post_time <= (now() AT TIME ZONE 'Europe/Moscow')::time
          AND NOT EXISTS (
            SELECT 1 FROM bot_chat_daily_deliveries d
            WHERE d.chat_id=c.chat_id
              AND d.local_date=(now() AT TIME ZONE 'Europe/Moscow')::date
              AND d.post_kind=post.post_kind
          )
        ORDER BY post.post_time, c.chat_id
        "#,
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|error| {
        tracing::error!("Could not load daily group posts: {}", error);
        Vec::new()
    });

    for (chat_id, group_id, local_date, thread_id, post_kind) in daily_chats {
        let reserved = sqlx::query(
            "INSERT INTO bot_chat_daily_deliveries(chat_id,local_date,post_kind) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
        )
        .bind(chat_id)
        .bind(local_date)
        .bind(&post_kind)
        .execute(&state.db)
        .await;
        if !matches!(reserved, Ok(ref row) if row.rows_affected() == 1) {
            continue;
        }

        let schedule = schedule_text(&state.db, group_id, "/today").await;
        let mut message = if post_kind == "digest" {
            format!("📬 Ежедневный дайджест учебной группы\n\n{schedule}")
        } else {
            format!("📢 Ежедневное расписание учебной группы\n\n{schedule}")
        };
        if post_kind == "digest" {
            let tasks = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
                "SELECT subject,due_at FROM bot_homework WHERE chat_id=$1 AND status='active' ORDER BY due_at NULLS LAST,id DESC LIMIT 8",
            )
            .bind(chat_id)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();
            message.push_str("\n\n📚 Активные задания:");
            if tasks.is_empty() {
                message.push_str("\nНа данный момент активных заданий нет.");
            } else {
                for (subject, due) in tasks {
                    let due = due.map(|value| moscow_time(value, "%d.%m %H:%M"))
                        .unwrap_or_else(|| "срок не указан".into());
                    message.push_str(&format!("\n• {} · {due}", escape_schedule_markdown(&subject)));
                }
            }
        }

        let request = bot.send_message(ChatId(chat_id), message).parse_mode(ParseMode::Markdown);
        let request = match thread_id.and_then(|id| i32::try_from(id).ok()) {
            Some(id) => request.message_thread_id(ThreadId(MessageId(id))),
            None => request,
        };
        match request.await {
            Ok(sent) => {
                if let Err(error) = sqlx::query(
                    "INSERT INTO bot_messages(telegram_id,chat_id,message_id) VALUES(0,$1,$2)",
                )
                .bind(chat_id)
                .bind(sent.id.0)
                .execute(&state.db)
                .await
                {
                    tracing::warn!("Could not track daily schedule message for replies: {}", error);
                }
                let _ = sqlx::query(
                    "DELETE FROM bot_messages WHERE telegram_id=0 AND chat_id=$1 AND id NOT IN (SELECT id FROM bot_messages WHERE telegram_id=0 AND chat_id=$1 ORDER BY id DESC LIMIT 100)",
                )
                .bind(chat_id)
                .execute(&state.db)
                .await;
                let _ = sqlx::query(
                    "UPDATE bot_chat_daily_deliveries SET delivered_at=now() WHERE chat_id=$1 AND local_date=$2 AND post_kind=$3",
                )
                .bind(chat_id)
                .bind(local_date)
                .bind(&post_kind)
                .execute(&state.db)
                .await;
                record_notification_delivery(
                    &state.db,
                    if post_kind == "digest" { "group_daily_digest" } else { "group_daily_schedule" },
                    "group",
                    chat_id,
                    true,
                    None,
                ).await;
            }
            Err(error) => {
                record_notification_delivery(
                    &state.db,
                    if post_kind == "digest" { "group_daily_digest" } else { "group_daily_schedule" },
                    "group",
                    chat_id,
                    false,
                    Some(&error.to_string()),
                ).await;
                tracing::warn!("Could not post {} to chat {}: {}", post_kind, chat_id, error);
                let _ = sqlx::query(
                    "DELETE FROM bot_chat_daily_deliveries WHERE chat_id=$1 AND local_date=$2 AND post_kind=$3",
                )
                .bind(chat_id)
                .bind(local_date)
                .bind(&post_kind)
                .execute(&state.db)
                .await;
            }
        }
    }

    let lessons = sqlx::query_as::<_, (i64, String, String, DateTime<Utc>, Option<i64>)>(
        r#"
        SELECT c.chat_id,l.id::text,l.subject,l.starts_at,c.message_thread_id
        FROM bot_chat_settings c
        JOIN lessons l ON l.group_id=c.group_id
        WHERE c.is_active=TRUE
          AND c.lesson_notifications_enabled=TRUE
          AND l.starts_at BETWEEN now()+interval '9 minutes'
                              AND now()+interval '10 minutes'
          AND NOT EXISTS (
            SELECT 1 FROM bot_chat_lesson_notifications n
            WHERE n.chat_id=c.chat_id AND n.lesson_id=l.id
          )
        ORDER BY l.starts_at
        "#,
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|error| {
        tracing::error!("Could not load group lesson notifications: {}", error);
        Vec::new()
    });

    for (chat_id, lesson_id, subject, starts_at, thread_id) in lessons {
        let Ok(lesson_uuid) = uuid::Uuid::parse_str(&lesson_id) else { continue; };
        let reserved = sqlx::query(
            "INSERT INTO bot_chat_lesson_notifications(chat_id,lesson_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(chat_id)
        .bind(lesson_uuid)
        .execute(&state.db)
        .await;
        if !matches!(reserved, Ok(ref row) if row.rows_affected()==1) { continue; }

        let text = format!(
            "🔔 Скоро начинается пара!\n\n📚 {}\n🕒 Начало: {}\n⏳ До начала примерно 10 минут",
            subject, moscow_time(starts_at, "%H:%M")
        );
        let request = bot.send_message(ChatId(chat_id), text);
        let request = match thread_id.and_then(|id| i32::try_from(id).ok()) {
            Some(id) => request.message_thread_id(ThreadId(MessageId(id))),
            None => request,
        };
        match request.await {
            Ok(_) => {
                record_notification_delivery(&state.db,"group_lesson_reminder","group",chat_id,true,None).await;
            }
            Err(error) => {
                record_notification_delivery(&state.db,"group_lesson_reminder","group",chat_id,false,Some(&error.to_string())).await;
                tracing::warn!("Could not send lesson notification to chat {}: {}",chat_id,error);
                let _ = sqlx::query(
                    "DELETE FROM bot_chat_lesson_notifications WHERE chat_id=$1 AND lesson_id=$2",
                )
                .bind(chat_id)
                .bind(lesson_uuid)
                .execute(&state.db)
                .await;
            }
        }
    }
}

/// Sends schedule-change notices only to students who enabled them in settings.
async fn schedule_change_tick(bot: &Bot, state: &Arc<AppState>) {
    let changes = sqlx::query_as::<_, (i64, uuid::Uuid, String, chrono::DateTime<Utc>, String, Option<String>, Option<String>, Option<String>, Option<String>)>(
        "SELECT id,group_id,change_kind,starts_at,subject,old_room,new_room,old_teacher,new_teacher FROM bot_schedule_changes WHERE created_at>now()-interval '7 days' ORDER BY id LIMIT 100"
    ).fetch_all(&state.db).await.unwrap_or_default();
    for (
        change_id,
        group_id,
        kind,
        starts_at,
        subject,
        old_room,
        new_room,
        old_teacher,
        new_teacher,
    ) in changes
    {
        let recipients = sqlx::query_scalar::<_, i64>(
            "SELECT telegram_id FROM users WHERE group_id=$1 AND COALESCE((settings->>'schedule_changes_enabled')::boolean,FALSE)=TRUE AND NOT EXISTS(SELECT 1 FROM bot_schedule_change_deliveries d WHERE d.change_id=$2 AND d.recipient_kind='private' AND d.recipient_id=users.telegram_id)"
        ).bind(group_id).bind(change_id).fetch_all(&state.db).await.unwrap_or_default();
        let when = moscow_time(starts_at, "%d.%m %H:%M");
        let details = match kind.as_str() {
            "room_changed" => format!(
                "Аудитория: {} → {}",
                old_room.as_deref().unwrap_or("не указана"),
                new_room.as_deref().unwrap_or("не указана")
            ),
            "teacher_changed" => format!(
                "Преподаватель: {} → {}",
                old_teacher.as_deref().unwrap_or("не указан"),
                new_teacher.as_deref().unwrap_or("не указан")
            ),
            _ => format!(
                "Занятие отменено{}{}",
                old_room
                    .as_deref()
                    .map(|v| format!(" · аудитория {v}"))
                    .unwrap_or_default(),
                old_teacher
                    .as_deref()
                    .map(|v| format!(" · {v}"))
                    .unwrap_or_default()
            ),
        };
        let text = format!("🔔 Изменение расписания\n{when} · {subject}\n{details}");
        for recipient in recipients {
            let reserved = sqlx::query("INSERT INTO bot_schedule_change_deliveries(change_id,recipient_kind,recipient_id) VALUES($1,'private',$2) ON CONFLICT DO NOTHING")
                .bind(change_id).bind(recipient).execute(&state.db).await.map(|r|r.rows_affected()==1).unwrap_or(false);
            if reserved {
                match bot.send_message(ChatId(recipient), &text).await {
                    Ok(_) => record_notification_delivery(&state.db,"schedule_change","private",recipient,true,None).await,
                    Err(error) => {
                        record_notification_delivery(&state.db,"schedule_change","private",recipient,false,Some(&error.to_string())).await;
                        let _ = sqlx::query("DELETE FROM bot_schedule_change_deliveries WHERE change_id=$1 AND recipient_kind='private' AND recipient_id=$2")
                            .bind(change_id).bind(recipient).execute(&state.db).await;
                    }
                }
            }
        }

        let group_chats = sqlx::query_as::<_, (i64, Option<i64>)>(
            r#"
            SELECT c.chat_id,c.message_thread_id
            FROM bot_chat_settings c
            WHERE c.group_id=$1 AND c.is_active=TRUE
              AND c.schedule_changes_enabled=TRUE
              AND NOT EXISTS(
                SELECT 1 FROM bot_schedule_change_deliveries d
                WHERE d.change_id=$2 AND d.recipient_kind='group' AND d.recipient_id=c.chat_id
              )
            ORDER BY c.chat_id
            "#
        )
        .bind(group_id)
        .bind(change_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        for (chat_id, thread_id) in group_chats {
            let reserved = sqlx::query(
                "INSERT INTO bot_schedule_change_deliveries(change_id,recipient_kind,recipient_id) VALUES($1,'group',$2) ON CONFLICT DO NOTHING",
            )
            .bind(change_id)
            .bind(chat_id)
            .execute(&state.db)
            .await
            .map(|row| row.rows_affected() == 1)
            .unwrap_or(false);
            if !reserved { continue; }

            let request = bot.send_message(ChatId(chat_id), &text);
            let request = match thread_id.and_then(|id| i32::try_from(id).ok()) {
                Some(id) => request.message_thread_id(ThreadId(MessageId(id))),
                None => request,
            };
            match request.await {
                Ok(_) => record_notification_delivery(&state.db,"schedule_change","group",chat_id,true,None).await,
                Err(error) => {
                    let detail = error.to_string();
                    record_notification_delivery(&state.db,"schedule_change","group",chat_id,false,Some(&detail)).await;
                    let _ = sqlx::query(
                        "DELETE FROM bot_schedule_change_deliveries WHERE change_id=$1 AND recipient_kind='group' AND recipient_id=$2",
                    )
                    .bind(change_id)
                    .bind(chat_id)
                    .execute(&state.db)
                    .await;
                }
            }
        }
    }
}

/// Sends one group-level homework reminder roughly 24 hours before each deadline.
/// A delivery key prevents duplicates when the 30-second worker ticks repeatedly.
async fn group_homework_reminder_tick(bot: &Bot, state: &Arc<AppState>) {
    let rows = sqlx::query_as::<_, (i64, i64, String, String, String, Option<i64>)>(
        r#"
        SELECT h.chat_id, h.id, h.subject, h.description, to_char(h.due_at AT TIME ZONE c.timezone, 'DD.MM.YYYY "в" HH24:MI') || ' (' || c.timezone || ')', c.message_thread_id
        FROM bot_homework h
        JOIN bot_chat_settings c ON c.chat_id = h.chat_id
        WHERE c.is_active = TRUE
          AND c.homework_reminders_enabled = TRUE
          AND h.status = 'active'
          AND h.due_at BETWEEN now() + interval '23 hours'
                           AND now() + interval '24 hours'
          AND NOT EXISTS (
              SELECT 1 FROM bot_group_reminder_deliveries d
              WHERE d.chat_id = h.chat_id
                AND d.record_type = 'homework'
                AND d.record_id = h.id
                AND d.telegram_id = 0
                AND d.reminder_key = 'day_before'
          )
        ORDER BY h.due_at
        "#
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|error| {
        tracing::error!("Could not load group homework reminders: {}", error);
        Vec::new()
    });

    for (chat_id, homework_id, subject, description, due_text, thread_id) in rows {
        let reserved = sqlx::query(
            "INSERT INTO bot_group_reminder_deliveries (chat_id, record_type, record_id, telegram_id, reminder_key) VALUES ($1, 'homework', $2, 0, 'day_before') ON CONFLICT DO NOTHING"
        )
        .bind(chat_id)
        .bind(homework_id)
        .execute(&state.db)
        .await;
        if !matches!(reserved, Ok(ref result) if result.rows_affected() == 1) {
            continue;
        }

        let message = format!(
            "📚 Напоминание о домашнем задании\n\nПредмет: {}\nЗадание: {}\nСрок: {}",
            subject, description, due_text
        );
        let request = bot.send_message(ChatId(chat_id), message);
        let request = match thread_id.and_then(|id| i32::try_from(id).ok()) {
            Some(id) => request.message_thread_id(ThreadId(MessageId(id))),
            None => request,
        };
        match request.await {
            Ok(_) => record_notification_delivery(
                &state.db, "group_homework_reminder", "group", chat_id, true, None,
            ).await,
            Err(error) => {
                let detail = error.to_string();
                record_notification_delivery(
                    &state.db, "group_homework_reminder", "group", chat_id, false, Some(&detail),
                ).await;
                tracing::warn!("Could not send homework reminder to chat {}: {}", chat_id, detail);
                let _ = sqlx::query(
                    "DELETE FROM bot_group_reminder_deliveries WHERE chat_id=$1 AND record_type='homework' AND record_id=$2 AND telegram_id=0 AND reminder_key='day_before'"
                )
                .bind(chat_id)
                .bind(homework_id)
                .execute(&state.db)
                .await;
            }
        }
    }
}

/// Sends exam and event reminders to the configured chat at the requested milestones.
async fn group_exam_event_reminder_tick(bot: &Bot, state: &Arc<AppState>) {
    // Exams are announced in the configured group; event reminders are private
    // and go only to participants who opted into reminders.
    let rows = sqlx::query_as::<_, (i64, String, i64, String, String, String, Option<i64>, i64)>(
        r#"
        SELECT c.chat_id, 'exam', e.id, e.subject, to_char(e.exam_at AT TIME ZONE c.timezone, 'DD.MM.YYYY HH24:MI') || ' (' || c.timezone || ')', r.key, c.message_thread_id, 0::bigint
        FROM bot_exams e
        JOIN bot_chat_settings c ON c.chat_id=e.chat_id
        CROSS JOIN (VALUES ('7d', interval '7 days'), ('3d', interval '3 days'), ('1d', interval '1 day')) AS r(key, lead)
        WHERE c.is_active=TRUE AND c.exam_reminders_enabled=TRUE AND e.status='active'
          AND e.exam_at BETWEEN now()+r.lead-interval '30 seconds' AND now()+r.lead
          AND NOT EXISTS (SELECT 1 FROM bot_group_reminder_deliveries d WHERE d.chat_id=e.chat_id AND d.record_type='exam' AND d.record_id=e.id AND d.telegram_id=0 AND d.reminder_key=r.key)
        UNION ALL
        SELECT c.chat_id, 'event', e.id, e.title, to_char(e.starts_at AT TIME ZONE c.timezone, 'DD.MM.YYYY HH24:MI') || ' (' || c.timezone || ')', r.key, NULL::bigint, p.telegram_id
        FROM bot_events e
        JOIN bot_chat_settings c ON c.chat_id=e.chat_id
        JOIN bot_event_participants p ON p.chat_id=e.chat_id AND p.event_id=e.id
        CROSS JOIN (VALUES ('3d', interval '3 days'), ('1d', interval '1 day'), ('2h', interval '2 hours')) AS r(key, lead)
        WHERE c.is_active=TRUE AND c.event_reminders_enabled=TRUE AND e.status='active'
          AND p.response='going' AND p.reminders_enabled=TRUE
          AND e.starts_at BETWEEN now()+r.lead-interval '30 seconds' AND now()+r.lead
          AND NOT EXISTS (SELECT 1 FROM bot_group_reminder_deliveries d WHERE d.chat_id=e.chat_id AND d.record_type='event' AND d.record_id=e.id AND d.telegram_id=p.telegram_id AND d.reminder_key=r.key)
        ORDER BY 5
        "#
    ).fetch_all(&state.db).await.unwrap_or_else(|error| {
        tracing::error!("Could not load exam/event reminders: {}", error);
        Vec::new()
    });

    for (chat_id, kind, record_id, title, at, key, thread_id, telegram_id) in rows {
        let reserved=sqlx::query("INSERT INTO bot_group_reminder_deliveries(chat_id,record_type,record_id,telegram_id,reminder_key) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
            .bind(chat_id).bind(&kind).bind(record_id).bind(telegram_id).bind(&key).execute(&state.db).await;
        if !matches!(reserved,Ok(ref r) if r.rows_affected()==1) {
            continue;
        }
        let label = if kind == "exam" {
            "экзамен"
        } else {
            "мероприятие"
        };
        let lead = match key.as_str() {
            "7d" => "7 дней",
            "3d" => "3 дня",
            "1d" => "1 день",
            "2h" => "2 часа",
            _ => "скоро",
        };
        let message = format!(
            "🔔 Напоминание: {} через {}\n\n{}\n🕒 {}",
            label, lead, title, at
        );
        let target = if kind == "event" {
            telegram_id
        } else {
            chat_id
        };
        let request = bot.send_message(ChatId(target), message);
        let request = if kind == "exam" {
            match thread_id.and_then(|id| i32::try_from(id).ok()) {
                Some(id) => request.message_thread_id(ThreadId(MessageId(id))),
                None => request,
            }
        } else {
            request
        };
        match request.await {
            Ok(_) => record_notification_delivery(
                &state.db,
                if kind == "exam" { "group_exam_reminder" } else { "personal_event_reminder" },
                if kind == "exam" { "group" } else { "private" },
                target,
                true,
                None,
            ).await,
            Err(error) => {
                let detail = error.to_string();
                record_notification_delivery(
                    &state.db,
                    if kind == "exam" { "group_exam_reminder" } else { "personal_event_reminder" },
                    if kind == "exam" { "group" } else { "private" },
                    target,
                    false,
                    Some(&detail),
                ).await;
                tracing::warn!("Could not send {} reminder to {}: {}", kind, target, detail);
                let _=sqlx::query("DELETE FROM bot_group_reminder_deliveries WHERE chat_id=$1 AND record_type=$2 AND record_id=$3 AND telegram_id=$4 AND reminder_key=$5")
                    .bind(chat_id).bind(&kind).bind(record_id).bind(telegram_id).bind(&key).execute(&state.db).await;
            }
        }
    }
}

/// Sends one personal morning digest at the time chosen in the user's settings.
async fn personal_digest_tick(bot: &Bot, state: &Arc<AppState>) {
    let recipients = sqlx::query_as::<_, (i64, uuid::Uuid, NaiveDate)>(
        r#"SELECT u.telegram_id,u.group_id,(now() AT TIME ZONE 'Europe/Moscow')::date
           FROM users u
           WHERE u.group_id IS NOT NULL
             AND COALESCE((u.settings->>'daily_digest_enabled')::boolean,FALSE)=TRUE
             AND COALESCE(NULLIF(u.settings->>'daily_digest_time','')::time,TIME '07:00') <= (now() AT TIME ZONE 'Europe/Moscow')::time
             AND NOT EXISTS (
                 SELECT 1 FROM bot_user_daily_digests d
                 WHERE d.telegram_id=u.telegram_id AND d.local_date=(now() AT TIME ZONE 'Europe/Moscow')::date
             )
           ORDER BY u.telegram_id"#
    ).fetch_all(&state.db).await.unwrap_or_else(|error| {
        tracing::error!("Could not load personal digest recipients: {}", error);
        Vec::new()
    });

    for (telegram_id, group_id, local_date) in recipients {
        let Some(next_day) = local_date.succ_opt() else {
            continue;
        };
        let Some(from) = moscow_offset()
            .from_local_datetime(&local_date.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .map(|v| v.with_timezone(&Utc))
        else {
            continue;
        };
        let Some(to) = moscow_offset()
            .from_local_datetime(&next_day.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .map(|v| v.with_timezone(&Utc))
        else {
            continue;
        };
        let reserved = sqlx::query("INSERT INTO bot_user_daily_digests(telegram_id,local_date) VALUES($1,$2) ON CONFLICT DO NOTHING")
            .bind(telegram_id).bind(local_date).execute(&state.db).await;
        if !matches!(reserved, Ok(ref row) if row.rows_affected()==1) {
            continue;
        }

        let lessons = sqlx::query_as::<_, (String,DateTime<Utc>,Option<String>)>(
            r#"SELECT l.subject,l.starts_at,r.name FROM lessons l LEFT JOIN rooms r ON r.id=l.room_id
               WHERE l.group_id=$1 AND l.starts_at >= $2 AND l.starts_at < $3 ORDER BY l.starts_at LIMIT 12"#
        ).bind(group_id).bind(from).bind(to).fetch_all(&state.db).await.unwrap_or_default();
        let tasks = sqlx::query_as::<_, (String,Option<DateTime<Utc>>)>(
            r#"SELECT h.subject,h.due_at FROM bot_homework h JOIN bot_chat_settings c ON c.chat_id=h.chat_id
               WHERE c.group_id=$1 AND h.status='active' AND h.due_at >= $2 AND h.due_at < $3
               ORDER BY h.due_at LIMIT 8"#
        ).bind(group_id).bind(from).bind(to).fetch_all(&state.db).await.unwrap_or_default();

        let mut text = format!("☀️ Твой дайджест на {}\n\n", local_date.format("%d.%m"));
        if lessons.is_empty() {
            text.push_str("🗓 Пар сегодня нет.\n");
        } else {
            text.push_str("🗓 Расписание:\n");
            for (subject, at, room) in lessons {
                text.push_str(&format!(
                    "• {} · {} · {}\n",
                    moscow_time(at, "%H:%M"),
                    subject,
                    room.unwrap_or_else(|| "аудитория не указана".into())
                ));
            }
        }
        if tasks.is_empty() {
            text.push_str("\n⏰ Дедлайнов на сегодня нет.");
        } else {
            text.push_str("\n⏰ Дедлайны сегодня:\n");
            for (subject, due) in tasks {
                text.push_str(&format!(
                    "• {} · {}\n",
                    subject,
                    due.map(|v| moscow_time(v, "%H:%M"))
                        .unwrap_or_else(|| "время не указано".into())
                ));
            }
        }
        match bot.send_message(ChatId(telegram_id), text).await {
            Ok(_) => record_notification_delivery(&state.db,"personal_daily_digest","private",telegram_id,true,None).await,
            Err(error) => {
                record_notification_delivery(&state.db,"personal_daily_digest","private",telegram_id,false,Some(&error.to_string())).await;
                tracing::warn!("Could not send personal digest to {}: {}",telegram_id,error);
                let _ = sqlx::query(
                    "DELETE FROM bot_user_daily_digests WHERE telegram_id=$1 AND local_date=$2",
                )
                .bind(telegram_id)
                .bind(local_date)
                .execute(&state.db)
                .await;
            }
        }
    }
}

async fn reminder_loop(bot: Bot, state: Arc<AppState>) {
    let mut tick = tokio::time::interval(Duration::from_secs(30));

    loop {
        tick.tick().await;

        if let Err(error) = sqlx::query(
            "INSERT INTO bot_notification_worker_state(worker,last_tick_at,updated_at) VALUES('telegram_reminders',now(),now()) ON CONFLICT(worker) DO UPDATE SET last_tick_at=now(),updated_at=now()",
        ).execute(&state.db).await {
            tracing::error!("Could not update notification worker heartbeat: {}", error);
        }

        let _ = sqlx::query("DELETE FROM bot_chat_mutes WHERE muted_until <= now()")
            .execute(&state.db)
            .await;
        let _ = sqlx::query(
            "DELETE FROM bot_chat_spam_seen WHERE last_seen_at < now()-interval '1 day'",
        )
        .execute(&state.db)
        .await;

        group_chat_schedule_loop_tick(&bot, &state).await;
        schedule_change_tick(&bot, &state).await;
        group_homework_reminder_tick(&bot, &state).await;
        group_exam_event_reminder_tick(&bot, &state).await;
        personal_digest_tick(&bot, &state).await;

        let rows = sqlx::query_as::<_, (i64, String, String, DateTime<Utc>, i32)>(
            r#"
            SELECT
                u.telegram_id,
                l.id::text,
                l.subject,
                l.starts_at,
                GREATEST(10, LEAST(1440, COALESCE(NULLIF(u.settings->>'reminder_minutes','')::integer,10)))
            FROM users u
            JOIN lessons l ON l.group_id = u.group_id
            WHERE COALESCE(
                (u.settings->>'reminders_enabled')::boolean,
                false
            ) = true
              AND l.starts_at BETWEEN now() + make_interval(mins => GREATEST(1,LEAST(1440,COALESCE(NULLIF(u.settings->>'reminder_minutes','')::integer,10)))-1)
                                  AND now() + make_interval(mins => GREATEST(10,LEAST(1440,COALESCE(NULLIF(u.settings->>'reminder_minutes','')::integer,10))))
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

        for (telegram_id, lesson_id, subject, starts_at, reminder_minutes) in rows {
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
                                    "⏰ Через {} мин. пара: {}\nНачало: {}",
                                    reminder_minutes,
                                    subject,
                                    moscow_time(starts_at, "%H:%M")
                                ),
                            )
                            .await
                        {
                            Ok(_) => record_notification_delivery(&state.db,"personal_lesson_reminder","private",telegram_id,true,None).await,
                            Err(error) => {
                                record_notification_delivery(&state.db,"personal_lesson_reminder","private",telegram_id,false,Some(&error.to_string())).await;
                                tracing::warn!("Could not deliver lesson reminder to {}: {}",telegram_id,error);
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
    use super::*;

    #[test]
    fn reply_keyboard_labels_are_not_treated_as_command_arguments() {
        for label in [
            "📚 ДЗ",
            "🗓 Расписание",
            "⏰ Дедлайны",
            "🏫 Аудитории",
            "👨‍🏫 Преподаватель",
            "⚙️ Настройки",
            "❓ Помощь",
            "👤 Кабинет",
            "💚 Знакомства",
            "📚 Задания",
        ] {
            assert_eq!(message_argument(label), "", "label: {label}");
        }
        assert_eq!(message_argument("/group ЛПб-ИСИТ-26-1"), "ЛПб-ИСИТ-26-1");
        assert_eq!(message_argument("/teacher Иванов"), "Иванов");
    }

    #[test]
    fn command_list_is_contextual_and_has_common_commands() {
        let private = commands_text(false);
        let group = commands_text(true);
        assert!(private.contains("/commands — этот список"));
        assert!(!private.contains("/add_dz"));
        assert!(group.contains("/add_dz"));
        assert!(group.contains("/report_queue"));
    }

    #[test]
    fn inline_search_understands_russian_schedule_queries() {
        assert_eq!(inline_actions("следующая пара")[0].id, "inline_next");
        assert_eq!(inline_actions("ближайшая пара")[0].id, "inline_next");
        assert_eq!(inline_actions("7 дней")[0].id, "inline_week");
        assert_eq!(inline_actions("завтра")[0].id, "inline_tomorrow");
    }

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
