//! Private student cabinet and opt-in adult matching. Public cards never show
//! Telegram usernames; contact links are sent only after a mutual like.

use sqlx::PgPool;
use teloxide::{
    prelude::*,
    types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, MessageId},
};

const TAGS: [(&str, &str); 6] = [
    ("study", "📚 Учёба"),
    ("coffee", "☕ Кофе"),
    ("walks", "🌿 Прогулки"),
    ("games", "🎮 Игры"),
    ("films", "🎬 Кино"),
    ("books", "📖 Книги"),
];

type SocialResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn button(label: impl Into<String>, action: impl Into<String>) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(label, action)
}

fn nav(action: &str) -> Vec<InlineKeyboardButton> {
    vec![button("⬅️ Назад", action), button("🏠 Меню", "ui:main")]
}

async fn edit_screen(
    bot: &Bot,
    chat_id: ChatId,
    message_id: MessageId,
    text: String,
    keyboard: InlineKeyboardMarkup,
) -> SocialResult<()> {
    tracing::info!(
        chat_id = chat_id.0,
        message_id = message_id.0,
        "Student social: editing message"
    );
    match bot
        .edit_message_text(chat_id, message_id, text)
        .reply_markup(keyboard)
        .await
    {
        Ok(_) => {
            tracing::info!("Student social: message edited successfully");
            Ok(())
        },
        // Repeated taps can request an identical screen. Telegram reports this
        // as an API error even though the desired screen is already displayed.
        Err(error) if error.to_string().contains("message is not modified") => {
            tracing::debug!("Student social: requested screen is already displayed");
            Ok(())
        }
        Err(error) => {
            tracing::error!("Student social: Telegram could not edit message: {}", error);
            Err(Box::new(error))
        },
    }
}

pub async fn open_cabinet(
    bot: &Bot,
    db: &PgPool,
    chat_id: ChatId,
    actor: i64,
) -> ResponseResult<()> {
    let (text, keyboard) = cabinet_screen(db, actor).await;
    bot.send_message(chat_id, text)
        .reply_markup(keyboard)
        .await?;
    Ok(())
}

pub async fn open_dating(
    bot: &Bot,
    db: &PgPool,
    chat_id: ChatId,
    actor: i64,
) -> ResponseResult<()> {
    let (text, keyboard) = dating_home(db, actor).await;
    bot.send_message(chat_id, text)
        .reply_markup(keyboard)
        .await?;
    Ok(())
}

pub async fn open_admin_reports(
    bot: &Bot,
    db: &PgPool,
    chat_id: ChatId,
    actor: i64,
    admin_id: i64,
) -> ResponseResult<()> {
    if admin_id <= 0 || actor != admin_id {
        bot.send_message(chat_id, "⛔ Очередь доступна только администратору бота.")
            .await?;
        return Ok(());
    }
    let (text, keyboard, _) = report_queue(db).await;
    bot.send_message(chat_id, text)
        .reply_markup(keyboard)
        .await?;
    Ok(())
}

pub async fn handle_callback(
    bot: &Bot,
    db: &PgPool,
    query: &CallbackQuery,
    admin_id: i64,
) -> ResponseResult<bool> {
    let Some(action) = query.data.as_deref() else {
        return Ok(false);
    };
    if !action.starts_with("pc:") && !action.starts_with("dating:") {
        return Ok(false);
    }
    let actor = i64::try_from(query.from.id.0).unwrap_or_default();

    // Telegram sends inline_message_id (instead of message) for messages
    // posted through inline mode. Such callbacks are valid and editable through
    // editMessageTextInline; treating them as missing messages breaks pagination.
    let Some(source_message) = query.message.as_ref() else {
        if let Some(inline_message_id) = query.inline_message_id.as_deref() {
            tracing::info!(
                action = action,
                actor = actor,
                inline_message_id = inline_message_id,
                "Student social callback: handling inline message"
            );
            bot.answer_callback_query(query.id.clone()).await?;

            // Only text-input flows need a separate private-chat message.
            // Non-interactive actions can be executed in-place and the source
            // inline message should remain the actual screen the user is using.
            if action == "pc:name" || action == "pc:cancel"
                || action == "dating:name" || action == "dating:bio"
                || action == "dating:photo"
            {
                let private_chat = ChatId(actor);
                if action == "pc:cancel" {
                    if let Err(error) = clear_flow(db, actor, actor).await {
                        tracing::error!("Could not clear private profile input flow: {}", error);
                        bot.answer_callback_query(query.id.clone())
                            .text("Не удалось отменить ввод. Попробуй ещё раз.")
                            .show_alert(true)
                            .await?;
                        return Ok(true);
                    }
                    let (screen, keyboard) = cabinet_screen(db, actor).await;
                    let _ = bot.send_message(private_chat, screen).reply_markup(keyboard).await;
                } else {
                    let prompt = match action {
                        "pc:name" => "✏️ Отправь новое имя для личного кабинета (2–48 символов).",
                        "dating:name" => "✏️ Отправь имя для анкеты знакомств (2–48 символов).",
                        "dating:bio" => "📝 Отправь описание анкеты (до 280 символов).",
                        "dating:photo" => "📷 Отправь фотографию следующим сообщением. Она будет прикреплена только к твоей анкете.",
                        _ => "Продолжи действие в личном чате.",
                    };
                    let flow = match action {
                        "pc:name" => Some("cabinet_name"),
                        "dating:name" => Some("dating_name"),
                        "dating:bio" => Some("dating_bio"),
                        "dating:photo" => Some("dating_photo"),
                        _ => None,
                    };
                    let placeholder = bot.send_message(private_chat, prompt).await;
                    if let Ok(message) = placeholder {
                        if let Some(flow) = flow {
                            if let Err(error) = start_flow(db, actor, actor, flow, message.id).await {
                                tracing::error!(action = action, "Could not start private input flow: {}", error);
                                let _ = bot.delete_message(private_chat, message.id).await;
                                bot.answer_callback_query(query.id.clone())
                                    .text("Не удалось начать ввод. Попробуй ещё раз.")
                                    .show_alert(true)
                                    .await?;
                                return Ok(true);
                            }
                        }
                    } else {
                        bot.answer_callback_query(query.id.clone())
                            .text("Открой личный чат с ботом командой /start, затем повтори действие.")
                            .show_alert(true)
                            .await?;
                        return Ok(true);
                    }
                }
                let status = "Запрос отправлен в этот личный чат.";
                let markup = InlineKeyboardMarkup::new(vec![vec![
                    button("👤 Личный кабинет", "pc:open"),
                    button("💚 Знакомства", "dating:open"),
                    button("🏠 Меню", "ui:main"),
                ]]);
                let _ = bot.edit_message_text_inline(inline_message_id, status)
                    .reply_markup(markup).await;
                return Ok(true);
            }

            if action.starts_with("dating:") {
                // Inline-mode messages only carry inline_message_id and cannot be
                // used as a normal chat message for multi-step dating screens.
                // Continue in the same private conversation, not in another chat.
                let private_chat = ChatId(actor);
                let message = match bot.send_message(
                    private_chat,
                    "Продолжение раздела знакомств находится в сообщении ниже — это тот же диалог с ботом.",
                ).await {
                    Ok(message) => message,
                    Err(error) => {
                        tracing::warn!(action = action, "Could not create same-chat dating screen: {}", error);
                        bot.answer_callback_query(query.id.clone())
                            .text("Открой личный чат с ботом командой /start, затем повтори действие.")
                            .show_alert(true)
                            .await?;
                        return Ok(true);
                    }
                };
                if let Err(error) = dating_action(
                    bot, db, actor, private_chat, message.id, action, admin_id,
                ).await {
                    tracing::error!(action = action, "Inline dating action failed: {}", error);
                    let _ = bot.edit_message_text(
                        private_chat,
                        message.id,
                        "Не удалось выполнить действие знакомств. Попробуй ещё раз через /dating.",
                    ).await;
                }
                let markup = InlineKeyboardMarkup::new(vec![vec![
                    button("💚 Знакомства", "dating:open"),
                    button("👤 Кабинет", "pc:open"),
                    button("🏠 Меню", "ui:main"),
                ]]);
                let _ = bot.edit_message_text_inline(
                    inline_message_id,
                    "Раздел знакомств открыт в сообщении ниже этого экрана.",
                ).reply_markup(markup).await;
                return Ok(true);
            }

            let screen = if action == "pc:open" {
                Some(Ok(cabinet_screen(db, actor).await))
            } else if action == "pc:groups" || action.starts_with("pc:groups:") {
                let page = action
                    .strip_prefix("pc:groups:")
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0)
                    .max(0);
                Some(groups_screen(db, page).await)
            } else if let Some(group_id) = action
                .strip_prefix("pc:group:")
                .and_then(|value| uuid::Uuid::parse_str(value).ok())
            {
                let changed = sqlx::query(
                    "UPDATE users SET group_id=$2,updated_at=now() WHERE telegram_id=$1 AND EXISTS(SELECT 1 FROM groups WHERE id=$2 AND is_active=TRUE)"
                )
                .bind(actor)
                .bind(group_id)
                .execute(db)
                .await;
                match changed {
                    Ok(result) if result.rows_affected() > 0 => Some(Ok(cabinet_screen(db, actor).await)),
                    Ok(_) => Some(Ok((
                        "Группа не найдена или больше недоступна.".into(),
                        cabinet_keyboard(),
                    ))),
                    Err(error) => Some(Err(Box::new(error) as Box<dyn std::error::Error + Send + Sync>)),
                }
            } else {
                None
            };

            if let Some(screen) = screen {
                let (text, keyboard) = match screen {
                    Ok(screen) => screen,
                    Err(error) => {
                        tracing::error!(
                            action = action,
                            "Could not build inline student social screen: {}",
                            error
                        );
                        bot.answer_callback_query(query.id.clone())
                            .text("Не удалось загрузить экран. Попробуй ещё раз.")
                            .show_alert(true)
                            .await?;
                        return Ok(true);
                    }
                };
                if let Err(error) = bot
                    .edit_message_text_inline(inline_message_id, text)
                    .reply_markup(keyboard)
                    .await
                {
                    if !error.to_string().contains("message is not modified") {
                        tracing::error!(
                            action = action,
                            "Could not edit inline student social message: {}",
                            error
                        );
                        bot.answer_callback_query(query.id.clone())
                            .text("Не удалось обновить экран. Попробуй ещё раз.")
                            .show_alert(true)
                            .await?;
                        return Ok(true);
                    }
                }
                tracing::info!(action = action, "Student social inline callback completed");
                return Ok(true);
            }

            tracing::warn!(
                action = action,
                "This student social action requires a regular private-chat message"
            );
            bot.answer_callback_query(query.id.clone())
                .text("Для этого действия открой личный кабинет командой /profile в личном чате с ботом.")
                .show_alert(true)
                .await?;
            return Ok(true);
        }

        tracing::error!(
            action = action,
            actor = actor,
            inline_message_id = ?query.inline_message_id,
            "Student social callback has neither a chat message nor inline message ID"
        );
        bot.answer_callback_query(query.id.clone())
            .text("Не удалось определить сообщение. Открой /profile заново.")
            .show_alert(true)
            .await?;
        return Ok(true);
    };
    let chat_id = source_message.chat().id;
    let message_id = source_message.id();

    if chat_id.0 != actor {
        tracing::warn!(
            action = action,
            actor = actor,
            chat_id = chat_id.0,
            "Student social callback rejected outside private chat"
        );
        bot.answer_callback_query(query.id.clone())
            .text("Личный кабинет и анкеты доступны только в личном чате с ботом.")
            .show_alert(true)
            .await?;
        return Ok(true);
    }
    tracing::info!(
        action = action,
        actor = actor,
        chat_id = chat_id.0,
        message_id = message_id.0,
        source_message = ?source_message,
        "Student social callback: dispatching action"
    );
    bot.answer_callback_query(query.id.clone()).await?;
    let result = if action.starts_with("pc:") {
        profile_action(bot, db, actor, chat_id, message_id, action).await
    } else {
        dating_action(
            bot,
            db,
            actor,
            chat_id,
            message_id,
            action,
            admin_id,
        )
        .await
    };
    match result {
        Ok(()) => tracing::info!(action = action, "Student social callback: action completed"),
        Err(error) => {
            tracing::error!(
                action = action,
                actor = actor,
                "Student social callback failed: {}",
                error
            );
            // Do not silently hide an action failure behind a second edit of the
            // same message. Send a separate diagnostic-facing reply instead.
            bot.send_message(
                chat_id,
                "Не удалось выполнить действие. Попробуй ещё раз. Если ошибка повторится, сообщи администратору.",
            )
            .await?;
        }
    }
    Ok(true)
}

async fn profile_action(
    bot: &Bot,
    db: &PgPool,
    actor: i64,
    chat_id: ChatId,
    message_id: MessageId,
    action: &str,
) -> SocialResult<()> {
    let (text, keyboard) = if action == "pc:open" {
        cabinet_screen(db, actor).await
    } else if action == "pc:groups" || action.starts_with("pc:groups:") {
        let page = action
            .strip_prefix("pc:groups:")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0)
            .max(0);
        groups_screen(db, page).await?
    } else if let Some(id) = action
        .strip_prefix("pc:group:")
        .and_then(|v| uuid::Uuid::parse_str(v).ok())
    {
        let changed = sqlx::query("UPDATE users SET group_id=$2,updated_at=now() WHERE telegram_id=$1 AND EXISTS(SELECT 1 FROM groups WHERE id=$2 AND is_active=TRUE)")
            .bind(actor).bind(id).execute(db).await?;
        if changed.rows_affected() == 0 {
            (
                "Группа не найдена или больше недоступна.".into(),
                cabinet_keyboard(),
            )
        } else {
            cabinet_screen(db, actor).await
        }
    } else if action == "pc:name" {
        start_flow(db, chat_id.0, actor, "cabinet_name", message_id).await?;
        ("✏️ Отправь новое имя для отображения в личном кабинете (2–48 символов). Оно не меняет имя Telegram.\n\nНажми «Отмена» или отправь /cancel, чтобы выйти.".into(),
            InlineKeyboardMarkup::new(vec![vec![button("Отмена", "pc:cancel")]]))
    } else if action == "pc:cancel" {
        clear_flow(db, chat_id.0, actor).await?;
        cabinet_screen(db, actor).await
    } else {
        (
            "Кнопка устарела. Открой кабинет заново.".into(),
            cabinet_keyboard(),
        )
    };
    edit_screen(bot, chat_id, message_id, text, keyboard).await?;
    Ok(())
}

fn cabinet_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![button("🎓 Выбрать группу", "pc:groups:0")],
        vec![button("✏️ Имя в кабинете", "pc:name")],
        vec![button("🔔 Уведомления", "ui:settings")],
        vec![button("💚 Знакомства", "dating:open")],
        nav("ui:main"),
    ])
}

async fn cabinet_screen(db: &PgPool, actor: i64) -> (String, InlineKeyboardMarkup) {
    let row = sqlx::query_as::<_, (String, Option<String>, serde_json::Value)>(
        "SELECT COALESCE(p.display_name,u.first_name,'Студент'),g.name,u.settings FROM users u LEFT JOIN bot_user_profiles p ON p.telegram_id=u.telegram_id LEFT JOIN groups g ON g.id=u.group_id WHERE u.telegram_id=$1"
    ).bind(actor).fetch_optional(db).await.ok().flatten();
    let Some((name, group, settings)) = row else {
        return (
            "Профиль пока не создан. Напиши /start в личном чате.".into(),
            cabinet_keyboard(),
        );
    };
    let text = format!("👤 ЛИЧНЫЙ КАБИНЕТ\n\nИмя: {name}\nУчебная группа: {}\nНапоминания: {}\nДайджест: {}\n\nНастройки приватные. В анкете знакомств имя и интересы показываются только участникам, которые сами включили знакомства.",
        group.unwrap_or_else(|| "не выбрана".into()),
        if settings["reminders_enabled"] == true { "включены" } else { "выключены" },
        if settings["daily_digest_enabled"] == true { settings["daily_digest_time"].as_str().unwrap_or("07:00").to_owned() } else { "выключен".into() });
    (text, cabinet_keyboard())
}

async fn groups_screen(db: &PgPool, page: i64) -> SocialResult<(String, InlineKeyboardMarkup)> {
    const PAGE_SIZE: i64 = 8;
    tracing::info!(page, "Student social: loading group picker page");
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM groups WHERE is_active=TRUE")
        .fetch_one(db)
        .await?;
    tracing::info!(total_groups = total, "Student social: active group count loaded");
    if total == 0 {
        return Ok((
            "В базе данных пока нет активных учебных групп. Сначала выполни синхронизацию списка групп, затем повтори /group.".into(),
            InlineKeyboardMarkup::new(vec![vec![
                button("⬅️ В кабинет", "pc:open"),
                button("🏠 Меню", "ui:main"),
            ]]),
        ));
    }
    let pages = (total + PAGE_SIZE - 1) / PAGE_SIZE;
    let page = page.clamp(0, pages - 1);
    let rows = sqlx::query_as::<_, (uuid::Uuid, String)>(
        "SELECT id,name FROM groups WHERE is_active=TRUE ORDER BY name LIMIT $1 OFFSET $2",
    )
    .bind(PAGE_SIZE)
    .bind(page * PAGE_SIZE)
    .fetch_all(db)
    .await?;
    tracing::info!(
        page,
        page_size = PAGE_SIZE,
        rows = rows.len(),
        "Student social: group picker rows loaded"
    );
    let mut keyboard = rows
        .into_iter()
        .map(|(id, name)| vec![button(name, format!("pc:group:{id}"))])
        .collect::<Vec<_>>();
    let mut paging = Vec::new();
    if page > 0 {
        paging.push(button("◀️ Назад", format!("pc:groups:{}", page - 1)));
    }
    if page + 1 < pages {
        paging.push(button("Далее ▶️", format!("pc:groups:{}", page + 1)));
    }
    // Page number is shown in the message text rather than as an inert button.
    if !paging.is_empty() {
        keyboard.push(paging);
    }
    keyboard.push(vec![
        button("⬅️ В кабинет", "pc:open"),
        button("🏠 Меню", "ui:main"),
    ]);
    Ok((
        format!("🎓 Выбери свою учебную группу. Показаны группы {}–{} из {}. Переключай страницы кнопками внизу.", page * PAGE_SIZE + 1, (page * PAGE_SIZE + PAGE_SIZE).min(total), total),
        InlineKeyboardMarkup::new(keyboard),
    ))
}

/// Opens the same paginated group picker used by the private cabinet.
/// This lets /group work without requiring a manually typed group name.
pub async fn open_group_picker(
    bot: &Bot,
    db: &PgPool,
    chat_id: ChatId,
) -> ResponseResult<()> {
    match groups_screen(db, 0).await {
        Ok((text, keyboard)) => {
            bot.send_message(chat_id, text).reply_markup(keyboard).await?;
        }
        Err(error) => {
            tracing::warn!("Could not load active groups for picker: {}", error);
            bot.send_message(chat_id, "Не удалось загрузить список групп из Neon PostgreSQL. Проверь подключение к БД и применение миграций.")
                .await?;
        }
    }
    Ok(())
}

async fn dating_home(db: &PgPool, actor: i64) -> (String, InlineKeyboardMarkup) {
    let profile = sqlx::query_as::<_, (String, String, Vec<String>, Option<chrono::DateTime<chrono::Utc>>, bool, bool, Option<String>, String, bool)>(
        "SELECT display_name,bio,tags,age_confirmed_at,is_active,is_paused,gender,preferred_gender,repeat_profiles_enabled FROM bot_dating_profiles WHERE telegram_id=$1"
    ).bind(actor).fetch_optional(db).await.ok().flatten();
    if let Some((name, bio, tags, age_confirmed, active, paused, gender, preferred_gender, repeat_profiles_enabled)) = profile {
        if age_confirmed.is_none() || !active {
            return dating_intro();
        }
        let incoming_likes: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM bot_dating_swipes s JOIN bot_dating_profiles p ON p.telegram_id=s.from_telegram_id WHERE s.to_telegram_id=$1 AND s.action='like' AND p.is_active=TRUE AND p.is_paused=FALSE AND p.age_confirmed_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM bot_dating_swipes mutual WHERE mutual.from_telegram_id=$1 AND mutual.to_telegram_id=s.from_telegram_id AND mutual.action='like') AND NOT EXISTS(SELECT 1 FROM bot_dating_blocks b WHERE (b.blocker_telegram_id=$1 AND b.blocked_telegram_id=s.from_telegram_id) OR (b.blocker_telegram_id=s.from_telegram_id AND b.blocked_telegram_id=$1))"
        ).bind(actor).fetch_one(db).await.unwrap_or(0);
        return dating_profile_screen(name, bio, tags, paused, incoming_likes, gender.as_deref(), &preferred_gender, repeat_profiles_enabled);
    }
    dating_intro()
}

fn dating_intro() -> (String, InlineKeyboardMarkup) {
    ("💚 ЗНАКОМСТВА\n\nДобровольные знакомства для общения, совместной учёбы и свиданий. Доступ только 18+ по самостоятельному подтверждению пользователя; бот не проверяет возраст по документам.\n\nПублично показываются только выбранное имя, короткое описание и интересы. Username и контакт не показываются; написать можно только после взаимного лайка. Можно поставить анкету на паузу или удалить её.".into(),
        InlineKeyboardMarkup::new(vec![
            vec![button("Мне есть 18 лет — создать анкету", "dating:consent")],
            vec![button("⬅️ Кабинет", "pc:open"), button("🏠 Меню", "ui:main")],
        ]))
}

fn tag_label(tag: &str) -> &str {
    TAGS.iter()
        .find(|(key, _)| *key == tag)
        .map(|(_, label)| *label)
        .unwrap_or("✨ Общение")
}

fn dating_profile_screen(
    name: String,
    bio: String,
    tags: Vec<String>,
    paused: bool,
    incoming_likes: i64,
    gender: Option<&str>,
    preferred_gender: &str,
    repeat_profiles_enabled: bool,
) -> (String, InlineKeyboardMarkup) {
    let tag_text = if tags.is_empty() {
        "пока не выбраны".into()
    } else {
        tags.iter()
            .map(|tag| tag_label(tag))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let gender_label = match gender { Some("male") => "мужской", Some("female") => "женский", _ => "не выбран" };
    let preference_label = match preferred_gender { "male" => "мужские", "female" => "женские", _ => "любые" };
    let text = format!("💚 МОЯ АНКЕТА\n\n{name} · 18+\nПол: {gender_label}\nПоказывать: {preference_label}\nПовтор анкет: {}\n{}\n\nИнтересы: {tag_text}\nСтатус: {}\n💚 Входящие лайки: {incoming_likes}\n\nАнкета показывается только взаимно подходящим участникам раздела знакомств.",
        if repeat_profiles_enabled { "включён (шанс 1/100)" } else { "выключен" },
        if bio.is_empty() { "Описание пока пустое" } else { &bio },
        if paused { "на паузе" } else { "активна" });
    let mut rows = vec![
        vec![
            button("🔎 Смотреть анкеты", "dating:discover"),
            button("💞 Мои совпадения", "dating:matches"),
        ],
        vec![
            button(format!("{} М", if gender == Some("male") { "✅" } else { " " }), "dating:gender:male"),
            button(format!("{} Ж", if gender == Some("female") { "✅" } else { " " }), "dating:gender:female"),
        ],
        vec![
            button(format!("{} Мужские", if preferred_gender == "male" { "✅" } else { " " }), "dating:pref:male"),
            button(format!("{} Женские", if preferred_gender == "female" { "✅" } else { " " }), "dating:pref:female"),
            button(format!("{} Все", if preferred_gender == "any" { "✅" } else { " " }), "dating:pref:any"),
        ],
        vec![button(if repeat_profiles_enabled { "🔁 Повтор анкет: ВКЛ" } else { "🔁 Повтор анкет: ВЫКЛ" }, "dating:repeat")],
        vec![
            button("✏️ Имя", "dating:name"),
            button("📝 Описание", "dating:bio"),
        ],
        vec![button(format!("💚 Кто меня лайкнул ({incoming_likes})"), "dating:likes")],
        vec![button("📷 Фото анкеты", "dating:photo")],
    ];
    for chunk in TAGS.chunks(2) {
        rows.push(
            chunk
                .iter()
                .map(|(key, label)| {
                    button(
                        format!(
                            "{}{}",
                            if tags.iter().any(|v| v == key) {
                                "✅ "
                            } else {
                                ""
                            },
                            label
                        ),
                        format!("dating:tag:{key}"),
                    )
                })
                .collect(),
        );
    }
    rows.push(vec![
        button(
            if paused {
                "▶️ Вернуть анкету"
            } else {
                "⏸ Пауза"
            },
            "dating:pause",
        ),
        button("🗑 Удалить анкету", "dating:delete"),
    ]);
    rows.push(nav("pc:open"));
    (text, InlineKeyboardMarkup::new(rows))
}

#[derive(Clone)]
struct Candidate {
    id: i64,
    name: String,
    bio: String,
    tags: Vec<String>,
    gender: Option<String>,
    photo_file_id: Option<String>,
}

async fn dating_action(
    bot: &Bot,
    db: &PgPool,
    actor: i64,
    chat_id: ChatId,
    message_id: MessageId,
    action: &str,
    admin_id: i64,
) -> SocialResult<()> {
    let mut displayed_photo: Option<String> = None;
    let (text, keyboard, notification) = if action == "dating:open" || action == "dating:profile" {
        let (text, kb) = dating_home(db, actor).await;
        (text, kb, None)
    } else if action == "dating:consent" {
        // Telegram first_name may be a single character. The database CHECK
        // requires a public dating display name between 2 and 48 characters,
        // so fall back safely rather than letting consent creation fail.
        let row = sqlx::query_as::<_, (String,)>(
            r#"
            SELECT COALESCE(
                CASE WHEN length(trim(p.display_name)) BETWEEN 2 AND 48 THEN trim(p.display_name) END,
                CASE WHEN length(trim(u.first_name)) BETWEEN 2 AND 48 THEN trim(u.first_name) END,
                'Студент'
            )
            FROM users u
            LEFT JOIN bot_user_profiles p ON p.telegram_id=u.telegram_id
            WHERE u.telegram_id=$1
            "#
        )
        .bind(actor)
        .fetch_optional(db)
        .await?
        .unwrap_or(("Студент".into(),));
        sqlx::query("INSERT INTO bot_dating_profiles(telegram_id,display_name,tags,age_confirmed_at) VALUES($1,$2,ARRAY['study']::text[],now()) ON CONFLICT(telegram_id) DO UPDATE SET age_confirmed_at=COALESCE(bot_dating_profiles.age_confirmed_at,now()),is_active=TRUE,is_paused=FALSE,updated_at=now()")
            .bind(actor).bind(row.0).execute(db).await?;
        let (text, kb) = dating_home(db, actor).await;
        (text, kb, None)
    } else if let Some(value) = action.strip_prefix("dating:gender:") {
        if !["male", "female"].contains(&value) {
            ("Некорректный выбор пола.".into(), InlineKeyboardMarkup::new(vec![nav("dating:profile")]), None)
        } else {
            sqlx::query("UPDATE bot_dating_profiles SET gender=$2,updated_at=now() WHERE telegram_id=$1 AND age_confirmed_at IS NOT NULL")
                .bind(actor).bind(value).execute(db).await?;
            let (text, kb) = dating_home(db, actor).await;
            (text, kb, None)
        }
    } else if let Some(value) = action.strip_prefix("dating:pref:") {
        if !["male", "female", "any"].contains(&value) {
            ("Некорректный фильтр пола.".into(), InlineKeyboardMarkup::new(vec![nav("dating:profile")]), None)
        } else {
            sqlx::query("UPDATE bot_dating_profiles SET preferred_gender=$2,updated_at=now() WHERE telegram_id=$1 AND age_confirmed_at IS NOT NULL")
                .bind(actor).bind(value).execute(db).await?;
            let (text, kb) = dating_home(db, actor).await;
            (text, kb, None)
        }
    } else if action == "dating:repeat" {
        sqlx::query("UPDATE bot_dating_profiles SET repeat_profiles_enabled=NOT repeat_profiles_enabled,updated_at=now() WHERE telegram_id=$1 AND age_confirmed_at IS NOT NULL")
            .bind(actor).execute(db).await?;
        let (text, kb) = dating_home(db, actor).await;
        (text, kb, None)
    } else if action == "dating:likes" {
        incoming_likes_screen(db, actor, 0).await?
    } else if let Some(page_text) = action.strip_prefix("dating:likes:") {
        let page = page_text.parse::<i64>().unwrap_or(0).clamp(0, 10_000);
        incoming_likes_screen(db, actor, page).await?
    } else if let Some(liker_text) = action.strip_prefix("dating:likeview:") {
        let mut parts = liker_text.split(':');
        let liker = parts.next().and_then(|value| value.parse::<i64>().ok()).unwrap_or_default();
        let page = parts.next().and_then(|value| value.parse::<i64>().ok()).unwrap_or(0).clamp(0, 10_000);
        incoming_like_card(db, actor, liker, page).await?
    } else if action == "dating:photo" {
        start_flow(db, chat_id.0, actor, "dating_photo", message_id).await?;
        (
            "📷 Отправь фотографию следующим сообщением в этот личный чат. Она будет показываться только в анкете знакомств. Не отправляй чужие фотографии без согласия человека.".into(),
            InlineKeyboardMarkup::new(vec![
                vec![button("🗑 Удалить фото", "dating:photo:delete")],
                nav("dating:profile"),
            ]),
            None,
        )
    } else if action == "dating:photo:delete" {
        sqlx::query("UPDATE bot_dating_profiles SET photo_file_id=NULL,updated_at=now() WHERE telegram_id=$1")
            .bind(actor).execute(db).await?;
        let (text, kb) = dating_home(db, actor).await;
        (text, kb, None)
    } else if action == "dating:pause" {
        sqlx::query("UPDATE bot_dating_profiles SET is_paused=NOT is_paused,updated_at=now() WHERE telegram_id=$1 AND age_confirmed_at IS NOT NULL AND is_active=TRUE").bind(actor).execute(db).await?;
        let (text, kb) = dating_home(db, actor).await;
        (text, kb, None)
    } else if action == "dating:delete" {
        (
            "Удалить анкету, совпадения и историю свайпов? Это действие нельзя отменить.".into(),
            InlineKeyboardMarkup::new(vec![vec![
                button("🗑 Да, удалить", "dating:delete:yes"),
                button("Отмена", "dating:profile"),
            ]]),
            None,
        )
    } else if action == "dating:delete:yes" {
        sqlx::query("DELETE FROM bot_dating_profiles WHERE telegram_id=$1")
            .bind(actor)
            .execute(db)
            .await?;
        dating_intro_with_text("Анкета удалена. Данные анкеты и история знакомств очищены.")
    } else if action == "dating:name" || action == "dating:bio" {
        start_flow(
            db,
            chat_id.0,
            actor,
            if action.ends_with("name") {
                "dating_name"
            } else {
                "dating_bio"
            },
            message_id,
        )
        .await?;
        let field = if action.ends_with("name") {
            "имя (2–48 символов)"
        } else {
            "описание (до 280 символов)"
        };
        (format!("✏️ Отправь новое {field}. Не добавляй телефон или другие личные контакты: username не публикуется.\n\nНажми «Отмена» или /cancel, чтобы выйти."),
            InlineKeyboardMarkup::new(vec![vec![button("Отмена","dating:cancel")]]),None)
    } else if action == "dating:cancel" {
        clear_flow(db, chat_id.0, actor).await?;
        let (text, kb) = dating_home(db, actor).await;
        (text, kb, None)
    } else if let Some(tag) = action.strip_prefix("dating:tag:") {
        if !TAGS.iter().any(|(key, _)| *key == tag) {
            (
                "Такого интереса нет.".into(),
                InlineKeyboardMarkup::new(vec![nav("dating:profile")]),
                None,
            )
        } else {
            sqlx::query("UPDATE bot_dating_profiles SET tags=CASE WHEN $2=ANY(tags) THEN array_remove(tags,$2) ELSE array_append(tags,$2) END,updated_at=now() WHERE telegram_id=$1 AND is_active=TRUE")
                .bind(actor).bind(tag).execute(db).await?;
            let (text, kb) = dating_home(db, actor).await;
            (text, kb, None)
        }
    } else if action == "dating:discover" || action == "dating:next" {
        match next_candidate(db, actor).await {
            Some(candidate) => {
                displayed_photo = candidate.photo_file_id.clone();
                (candidate_text(&candidate), candidate_keyboard(candidate.id), None)
            },
            None => (
                "Пока новых анкет нет. Попробуй позже или измени интересы в своей анкете.".into(),
                InlineKeyboardMarkup::new(vec![
                    vec![button("👤 Моя анкета", "dating:profile")],
                    nav("pc:open"),
                ]),
                None,
            ),
        }
    } else if action == "dating:prev" {
        match previous_candidate(db, actor).await {
            Some(candidate) => {
                displayed_photo = candidate.photo_file_id.clone();
                (candidate_text(&candidate), candidate_keyboard(candidate.id), None)
            }
            None => ("Предыдущей анкеты пока нет.".into(), InlineKeyboardMarkup::new(vec![vec![button("🔎 Смотреть анкеты", "dating:discover")], nav("dating:profile")]), None),
        }
    } else if action.starts_with("dating:like:") || action.starts_with("dating:like_incoming:") || action.starts_with("dating:pass:") {
        let incoming_like = action.starts_with("dating:like_incoming:");
        let is_like = !action.starts_with("dating:pass:");
        let suffix = if incoming_like {
            action.strip_prefix("dating:like_incoming:").unwrap_or_default()
        } else if is_like {
            action.strip_prefix("dating:like:").unwrap_or_default()
        } else {
            action.strip_prefix("dating:pass:").unwrap_or_default()
        };
        let mut parts = suffix.split(':');
        let target = parts.next().and_then(|v| v.parse::<i64>().ok()).filter(|id| *id > 0);
        let incoming_page = parts.next().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0).clamp(0, 10_000);
        let Some(target) = target else {
            return Ok(());
        };
        let result = sqlx::query("INSERT INTO bot_dating_swipes(from_telegram_id,to_telegram_id,action) SELECT $1,$2,$3 WHERE EXISTS(SELECT 1 FROM bot_dating_profiles WHERE telegram_id=$1 AND is_active=TRUE AND is_paused=FALSE AND age_confirmed_at IS NOT NULL) AND EXISTS(SELECT 1 FROM bot_dating_profiles WHERE telegram_id=$2 AND is_active=TRUE AND is_paused=FALSE AND age_confirmed_at IS NOT NULL) AND (NOT $4 OR EXISTS(SELECT 1 FROM bot_dating_swipes WHERE from_telegram_id=$2 AND to_telegram_id=$1 AND action='like')) ON CONFLICT(from_telegram_id,to_telegram_id) DO UPDATE SET action=EXCLUDED.action,created_at=now()")
            .bind(actor).bind(target).bind(if is_like{"like"}else{"pass"}).bind(incoming_like).execute(db).await?;
        if result.rows_affected() == 0 {
            ("Этот лайк уже недоступен. Список обновлён.".into(), InlineKeyboardMarkup::new(vec![vec![button("💚 Входящие лайки","dating:likes")],nav("dating:profile")]), None)
        } else if is_like && sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM bot_dating_swipes WHERE from_telegram_id=$2 AND to_telegram_id=$1 AND action='like') AND NOT EXISTS(SELECT 1 FROM bot_dating_blocks WHERE (blocker_telegram_id=$1 AND blocked_telegram_id=$2) OR (blocker_telegram_id=$2 AND blocked_telegram_id=$1))")
            .bind(actor).bind(target).fetch_one(db).await.unwrap_or(false) {
            let (one,two)=if actor<target {(actor,target)} else {(target,actor)};
            sqlx::query("INSERT INTO bot_dating_matches(user1_telegram_id,user2_telegram_id,status) VALUES($1,$2,'active') ON CONFLICT(user1_telegram_id,user2_telegram_id) DO UPDATE SET status='active',created_at=now(),ended_at=NULL")
                .bind(one).bind(two).execute(db).await?;
            let target_profile=profile_short(db,target).await;
            let name=target_profile.map(|v|v.0).unwrap_or_else(||"новый знакомый".into());
            (format!("💞 Взаимный лайк! Вы понравились друг другу.\n\n{name} · Написать можно только по взаимному согласию."),InlineKeyboardMarkup::new(vec![vec![contact_button(target)],vec![button("Мои совпадения","dating:matches")],nav("dating:profile")]),Some((target,format!("💞 У вас взаимный лайк с {}! Чтобы начать общение, нажми кнопку ниже.",profile_short(db,actor).await.map(|v|v.0).unwrap_or_else(||"студентом".into())))))
        } else if incoming_like {
            let (text, keyboard, _) = incoming_likes_screen(db, actor, incoming_page).await?;
            (text, keyboard, None)
        } else {
            let (text,kb)=match next_candidate(db,actor).await {
                Some(c) => { displayed_photo = c.photo_file_id.clone(); (candidate_text(&c),candidate_keyboard(c.id)) },
                None => ("Новых анкет сейчас нет.".into(),InlineKeyboardMarkup::new(vec![vec![button("Моя анкета","dating:profile")]]))
            };
            (if is_like { format!("👍 Лайк отправлен.\n\n{text}") } else { text },kb,None)
        }
    } else if let Some(target_text) = action.strip_prefix("dating:reportmenu:") {
        let target = target_text.parse::<i64>().unwrap_or_default();
        (
            "Почему ты хочешь пожаловаться на анкету? После жалобы анкета будет скрыта от тебя."
                .into(),
            InlineKeyboardMarkup::new(vec![
                vec![
                    button("Спам", format!("dating:report:{target}:spam")),
                    button(
                        "Неподходящее",
                        format!("dating:report:{target}:inappropriate"),
                    ),
                ],
                vec![
                    button(
                        "Возможно, младше 18",
                        format!("dating:report:{target}:underage"),
                    ),
                    button("Другое", format!("dating:report:{target}:other")),
                ],
                vec![button("⬅️ Назад к анкете", format!("dating:discover"))],
            ]),
            None,
        )
    } else if action.starts_with("dating:report:") {
        let parts = action.split(':').collect::<Vec<_>>();
        let target = parts
            .get(2)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or_default();
        let category = parts.get(3).copied().unwrap_or("other");
        if target <= 0 || !["spam", "inappropriate", "underage", "other"].contains(&category) {
            (
                "Жалоба не обработана.".into(),
                InlineKeyboardMarkup::new(vec![nav("dating:profile")]),
                None,
            )
        } else {
            sqlx::query("INSERT INTO bot_dating_reports(reporter_telegram_id,target_telegram_id,category) VALUES($1,$2,$3)").bind(actor).bind(target).bind(category).execute(db).await?;
            block_user(db, actor, target).await?;
            if admin_id > 0 {
                let _=bot.send_message(ChatId(admin_id),format!("Новая жалоба на анкету #{target}: {category}. Очередь: /dating_reports")).await;
            }
            (
                "Жалоба отправлена. Анкета скрыта, пользователь заблокирован для твоего профиля."
                    .into(),
                InlineKeyboardMarkup::new(vec![nav("dating:profile")]),
                None,
            )
        }
    } else if let Some(target_text) = action.strip_prefix("dating:block:") {
        let target = target_text.parse::<i64>().unwrap_or_default();
        if target > 0 {
            block_user(db, actor, target).await?;
        }
        (
            "Анкета скрыта. Пользователь больше не появится в подборках.".into(),
            InlineKeyboardMarkup::new(vec![
                vec![button("Продолжить поиск", "dating:discover")],
                nav("dating:profile"),
            ]),
            None,
        )
    } else if action == "dating:matches" {
        matches_screen(db, actor).await
    } else if let Some(id_text) = action.strip_prefix("dating:unmatch:") {
        let other = id_text.parse::<i64>().unwrap_or_default();
        let (one, two) = if actor < other {
            (actor, other)
        } else {
            (other, actor)
        };
        sqlx::query("UPDATE bot_dating_matches SET status='ended',ended_at=now() WHERE user1_telegram_id=$1 AND user2_telegram_id=$2 AND status='active'").bind(one).bind(two).execute(db).await?;
        sqlx::query("DELETE FROM bot_dating_swipes WHERE (from_telegram_id=$1 AND to_telegram_id=$2) OR (from_telegram_id=$2 AND to_telegram_id=$1)").bind(actor).bind(other).execute(db).await?;
        matches_screen(db, actor).await
    } else if action == "dating:adminqueue" {
        if actor != admin_id || admin_id <= 0 {
            (
                "Недостаточно прав.".into(),
                InlineKeyboardMarkup::new(vec![nav("ui:main")]),
                None,
            )
        } else {
            report_queue(db).await
        }
    } else if let Some(rest) = action.strip_prefix("dating:reportdone:") {
        if actor != admin_id || admin_id <= 0 {
            (
                "Недостаточно прав.".into(),
                InlineKeyboardMarkup::new(vec![nav("ui:main")]),
                None,
            )
        } else {
            let mut p = rest.split(':');
            let id = p
                .next()
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or_default();
            let status = p.next().unwrap_or("reviewed");
            if id > 0 && ["reviewed", "dismissed"].contains(&status) {
                sqlx::query("UPDATE bot_dating_reports SET status=$2,handled_by=$3 WHERE id=$1 AND status='open'").bind(id).bind(status).bind(actor).execute(db).await?;
            }
            report_queue(db).await
        }
    } else {
        (
            "Кнопка устарела.".into(),
            InlineKeyboardMarkup::new(vec![nav("dating:profile")]),
            None,
        )
    };

    edit_screen(bot, chat_id, message_id, text, keyboard).await?;
    if let Some((target, message_text)) = notification {
        let _ = bot
            .send_message(ChatId(target), message_text)
            .reply_markup(InlineKeyboardMarkup::new(vec![vec![contact_button(actor)]]))
            .await;
    }

    // Display a candidate's optional Telegram-hosted photo as a second message.
    // The file_id is private Telegram storage; no third-party image host is used.
    if let Some(file_id) = displayed_photo {
        bot.send_photo(chat_id, teloxide::types::InputFile::file_id(file_id))
            .caption("📷 Фото анкеты")
            .await?;
    }
    if let Some(liker_text) = action.strip_prefix("dating:likeview:") {
        let liker = liker_text.split(':').next().and_then(|value| value.parse::<i64>().ok()).unwrap_or_default();
        let photo: Option<String> = sqlx::query_scalar(
            "SELECT p.photo_file_id FROM bot_dating_swipes s JOIN bot_dating_profiles p ON p.telegram_id=s.from_telegram_id WHERE s.to_telegram_id=$1 AND s.from_telegram_id=$2 AND s.action='like' AND p.is_active=TRUE AND p.is_paused=FALSE AND p.age_confirmed_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM bot_dating_swipes mutual WHERE mutual.from_telegram_id=$1 AND mutual.to_telegram_id=s.from_telegram_id AND mutual.action='like') AND NOT EXISTS(SELECT 1 FROM bot_dating_blocks b WHERE (b.blocker_telegram_id=$1 AND b.blocked_telegram_id=$2) OR (b.blocker_telegram_id=$2 AND b.blocked_telegram_id=$1))"
        ).bind(actor).bind(liker).fetch_optional(db).await?.flatten();
        if let Some(file_id) = photo {
            bot.send_photo(chat_id, teloxide::types::InputFile::file_id(file_id))
                .caption("📷 Фото анкеты")
                .await?;
        }
    }
    Ok(())
}

fn dating_intro_with_text(text: &str) -> (String, InlineKeyboardMarkup, Option<(i64, String)>) {
    let (_, keyboard) = dating_intro();
    (text.into(), keyboard, None)
}

fn candidate_keyboard(id: i64) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            button("💚 Нравится", format!("dating:like:{id}")),
            button("➡️ Дальше", format!("dating:pass:{id}")),
        ],
        vec![
            button("🚩 Пожаловаться", format!("dating:reportmenu:{id}")),
            button("🚫 Скрыть", format!("dating:block:{id}")),
        ],
        vec![button("⬅️ Предыдущая", "dating:prev"), button("Следующая ➡️", "dating:next")],
        nav("dating:profile"),
    ])
}

fn candidate_text(candidate: &Candidate) -> String {
    let interests = if candidate.tags.is_empty() {
        "не указаны".into()
    } else {
        candidate.tags.iter().map(|tag| tag_label(tag)).collect::<Vec<_>>().join(" · ")
    };
    let gender = match candidate.gender.as_deref() { Some("male") => "Мужской", Some("female") => "Женский", _ => "Не указан" };
    format!(
        "💚 АНКЕТА\\n\\n{} · 18+\\nПол: {}\\n{}\\n\\nИнтересы: {}\\n\\nUsername будет скрыт; контакт откроется только после взаимного лайка.",
        candidate.name,
        gender,
        if candidate.bio.is_empty() { "Без описания" } else { &candidate.bio },
        interests
    )
}

async fn next_candidate(db: &PgPool, actor: i64) -> Option<Candidate> {
    // One random draw per request: 99% unseen pool, 1% previously viewed pool
    // when the user explicitly enables repeats.
    let candidate = sqlx::query_as::<_,(i64,String,String,Vec<String>,Option<String>,Option<String>)>(
        "WITH draw AS (SELECT random() < 0.01 AS repeat_draw)          SELECT p.telegram_id,p.display_name,p.bio,p.tags,p.gender,p.photo_file_id          FROM bot_dating_profiles own CROSS JOIN bot_dating_profiles p CROSS JOIN draw          WHERE own.telegram_id=$1 AND own.is_active=TRUE AND own.is_paused=FALSE AND own.age_confirmed_at IS NOT NULL          AND p.telegram_id<>own.telegram_id AND p.telegram_id<>COALESCE((SELECT current_profile_id FROM bot_dating_browse_state WHERE viewer_telegram_id=own.telegram_id),-1) AND p.is_active=TRUE AND p.is_paused=FALSE AND p.age_confirmed_at IS NOT NULL          AND (own.preferred_gender='any' OR p.gender=own.preferred_gender)          AND ((NOT EXISTS(SELECT 1 FROM bot_dating_seen_profiles seen WHERE seen.viewer_telegram_id=own.telegram_id AND seen.viewed_telegram_id=p.telegram_id) AND NOT draw.repeat_draw)               OR (own.repeat_profiles_enabled=TRUE AND EXISTS(SELECT 1 FROM bot_dating_seen_profiles seen WHERE seen.viewer_telegram_id=own.telegram_id AND seen.viewed_telegram_id=p.telegram_id) AND draw.repeat_draw))          AND NOT EXISTS(SELECT 1 FROM bot_dating_blocks b WHERE (b.blocker_telegram_id=own.telegram_id AND b.blocked_telegram_id=p.telegram_id) OR (b.blocker_telegram_id=p.telegram_id AND b.blocked_telegram_id=own.telegram_id))          AND NOT EXISTS(SELECT 1 FROM bot_dating_matches m WHERE m.status='active' AND ((m.user1_telegram_id=own.telegram_id AND m.user2_telegram_id=p.telegram_id) OR (m.user1_telegram_id=p.telegram_id AND m.user2_telegram_id=own.telegram_id)))          ORDER BY random() LIMIT 1"
    ).bind(actor).fetch_optional(db).await.ok().flatten()
      .map(|(id,name,bio,tags,gender,photo_file_id)| Candidate{id,name,bio,tags,gender,photo_file_id})?;
    if remember_candidate(db, actor, candidate.id).await.is_err() {
        tracing::error!("Could not persist dating browse history for user {}", actor);
        return None;
    }
    Some(candidate)
}

async fn remember_candidate(db: &PgPool, actor: i64, candidate_id: i64) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query("INSERT INTO bot_dating_seen_profiles(viewer_telegram_id,viewed_telegram_id,last_seen_at) VALUES($1,$2,now()) ON CONFLICT(viewer_telegram_id,viewed_telegram_id) DO UPDATE SET last_seen_at=now()")
        .bind(actor).bind(candidate_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO bot_dating_browse_state(viewer_telegram_id,current_profile_id) VALUES($1,$2) ON CONFLICT(viewer_telegram_id) DO UPDATE SET previous_profile_id=bot_dating_browse_state.current_profile_id,current_profile_id=EXCLUDED.current_profile_id,updated_at=now()")
        .bind(actor).bind(candidate_id).execute(&mut *tx).await?;
    tx.commit().await
}

async fn previous_candidate(db: &PgPool, actor: i64) -> Option<Candidate> {
    let candidate = sqlx::query_as::<_,(i64,String,String,Vec<String>,Option<String>,Option<String>)>(
        "SELECT p.telegram_id,p.display_name,p.bio,p.tags,p.gender,p.photo_file_id          FROM bot_dating_browse_state state JOIN bot_dating_profiles p ON p.telegram_id=state.previous_profile_id          JOIN bot_dating_profiles own ON own.telegram_id=state.viewer_telegram_id          WHERE state.viewer_telegram_id=$1 AND own.is_active=TRUE AND own.is_paused=FALSE AND own.age_confirmed_at IS NOT NULL          AND p.is_active=TRUE AND p.is_paused=FALSE AND p.age_confirmed_at IS NOT NULL          AND (own.preferred_gender='any' OR p.gender=own.preferred_gender)          AND NOT EXISTS(SELECT 1 FROM bot_dating_blocks b WHERE (b.blocker_telegram_id=own.telegram_id AND b.blocked_telegram_id=p.telegram_id) OR (b.blocker_telegram_id=p.telegram_id AND b.blocked_telegram_id=own.telegram_id))"
    ).bind(actor).fetch_optional(db).await.ok().flatten()
      .map(|(id,name,bio,tags,gender,photo_file_id)| Candidate{id,name,bio,tags,gender,photo_file_id})?;
    sqlx::query("UPDATE bot_dating_browse_state SET previous_profile_id=current_profile_id,current_profile_id=$2,updated_at=now() WHERE viewer_telegram_id=$1")
        .bind(actor).bind(candidate.id).execute(db).await.ok()?;
    Some(candidate)
}

async fn profile_short(db: &PgPool, id: i64) -> Option<(String, String)> {
    sqlx::query_as("SELECT display_name,bio FROM bot_dating_profiles WHERE telegram_id=$1 AND is_active=TRUE AND is_paused=FALSE AND age_confirmed_at IS NOT NULL").bind(id).fetch_optional(db).await.ok().flatten()
}

fn contact_button(id: i64) -> InlineKeyboardButton {
    let url =
        url::Url::parse(&format!("tg://user?id={id}")).expect("valid Telegram user deep link");
    InlineKeyboardButton::url("✉️ Написать", url)
}

async fn block_user(db: &PgPool, actor: i64, target: i64) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO bot_dating_blocks(blocker_telegram_id,blocked_telegram_id) SELECT $1,$2 WHERE EXISTS(SELECT 1 FROM bot_dating_profiles WHERE telegram_id=$1) AND EXISTS(SELECT 1 FROM bot_dating_profiles WHERE telegram_id=$2) ON CONFLICT DO NOTHING").bind(actor).bind(target).execute(db).await?;
    let (one, two) = if actor < target {
        (actor, target)
    } else {
        (target, actor)
    };
    sqlx::query("UPDATE bot_dating_matches SET status='ended',ended_at=now() WHERE user1_telegram_id=$1 AND user2_telegram_id=$2 AND status='active'").bind(one).bind(two).execute(db).await?;
    Ok(())
}

async fn incoming_likes_screen(
    db: &PgPool,
    actor: i64,
    page: i64,
) -> SocialResult<(String, InlineKeyboardMarkup, Option<(i64, String)>)> {
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bot_dating_swipes s JOIN bot_dating_profiles p ON p.telegram_id=s.from_telegram_id WHERE s.to_telegram_id=$1 AND s.action='like' AND p.is_active=TRUE AND p.is_paused=FALSE AND p.age_confirmed_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM bot_dating_swipes mutual WHERE mutual.from_telegram_id=$1 AND mutual.to_telegram_id=s.from_telegram_id AND mutual.action='like') AND NOT EXISTS(SELECT 1 FROM bot_dating_blocks b WHERE (b.blocker_telegram_id=$1 AND b.blocked_telegram_id=s.from_telegram_id) OR (b.blocker_telegram_id=s.from_telegram_id AND b.blocked_telegram_id=$1))"
    ).bind(actor).fetch_one(db).await?;
    let pages = total.max(1);
    let page = page.clamp(0, pages - 1);
    let liker = sqlx::query_scalar::<_, i64>(
        "SELECT s.from_telegram_id FROM bot_dating_swipes s JOIN bot_dating_profiles p ON p.telegram_id=s.from_telegram_id WHERE s.to_telegram_id=$1 AND s.action='like' AND p.is_active=TRUE AND p.is_paused=FALSE AND p.age_confirmed_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM bot_dating_swipes mutual WHERE mutual.from_telegram_id=$1 AND mutual.to_telegram_id=s.from_telegram_id AND mutual.action='like') AND NOT EXISTS(SELECT 1 FROM bot_dating_blocks b WHERE (b.blocker_telegram_id=$1 AND b.blocked_telegram_id=s.from_telegram_id) OR (b.blocker_telegram_id=s.from_telegram_id AND b.blocked_telegram_id=$1)) ORDER BY s.created_at DESC, s.from_telegram_id DESC LIMIT 1 OFFSET $2"
    ).bind(actor).bind(page).fetch_optional(db).await?;
    let mut rows = Vec::new();
    if let Some(liker) = liker {
        rows.push(vec![button("Открыть анкету", format!("dating:likeview:{liker}:{page}"))]);
    }
    if page > 0 {
        rows.push(vec![button("◀️ Предыдущий лайк", format!("dating:likes:{}", page - 1))]);
    }
    if page + 1 < pages {
        rows.push(vec![button("Следующий лайк ▶️", format!("dating:likes:{}", page + 1))]);
    }
    rows.push(vec![button("⬅️ К анкете", "dating:profile"), button("🏠 Меню", "ui:main")]);
    Ok((
        format!("💚 Тебя лайкнули: {total}.\n{}",
            if total == 0 { "Пока нет входящих лайков.".to_owned() } else { format!("Анкета {} из {} доступна для просмотра.", page + 1, total) }),
        InlineKeyboardMarkup::new(rows),
        None,
    ))
}

async fn incoming_like_card(
    db: &PgPool,
    actor: i64,
    liker: i64,
    page: i64,
) -> SocialResult<(String, InlineKeyboardMarkup, Option<(i64, String)>)> {
    let card = sqlx::query_as::<_, (String, String, Vec<String>, Option<String>)>(
        "SELECT p.display_name,p.bio,p.tags,p.photo_file_id FROM bot_dating_swipes s JOIN bot_dating_profiles p ON p.telegram_id=s.from_telegram_id WHERE s.to_telegram_id=$1 AND s.from_telegram_id=$2 AND s.action='like' AND p.is_active=TRUE AND p.is_paused=FALSE AND p.age_confirmed_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM bot_dating_swipes mutual WHERE mutual.from_telegram_id=$1 AND mutual.to_telegram_id=s.from_telegram_id AND mutual.action='like') AND NOT EXISTS(SELECT 1 FROM bot_dating_blocks b WHERE (b.blocker_telegram_id=$1 AND b.blocked_telegram_id=$2) OR (b.blocker_telegram_id=$2 AND b.blocked_telegram_id=$1))"
    ).bind(actor).bind(liker).fetch_optional(db).await?;
    let Some((name, bio, tags, photo_file_id)) = card else {
        return Ok((
            "Этот лайк больше недоступен.".into(),
            InlineKeyboardMarkup::new(vec![nav("dating:likes")]),
            None,
        ));
    };
    let interests = if tags.is_empty() { "не указаны".to_owned() } else { tags.iter().map(|tag| tag_label(tag)).collect::<Vec<_>>().join(" · ") };
    let text = format!("💚 ТЕБЯ ЛАЙКНУЛИ\n\n{name} · 18+\n{}\n\nИнтересы: {interests}\n\nЕсли симпатия взаимная, поставь лайк в ответ.", if bio.is_empty() { "Без описания" } else { &bio });

    // Keep card navigation aligned with the same filtered incoming-like list:
    // inactive/paused profiles, mutual likes and blocked users are not browsable.
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bot_dating_swipes s JOIN bot_dating_profiles p ON p.telegram_id=s.from_telegram_id WHERE s.to_telegram_id=$1 AND s.action='like' AND p.is_active=TRUE AND p.is_paused=FALSE AND p.age_confirmed_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM bot_dating_swipes mutual WHERE mutual.from_telegram_id=$1 AND mutual.to_telegram_id=s.from_telegram_id AND mutual.action='like') AND NOT EXISTS(SELECT 1 FROM bot_dating_blocks b WHERE (b.blocker_telegram_id=$1 AND b.blocked_telegram_id=s.from_telegram_id) OR (b.blocker_telegram_id=s.from_telegram_id AND b.blocked_telegram_id=$1))"
    )
    .bind(actor)
    .fetch_one(db)
    .await?;
    let previous = if page > 0 {
        sqlx::query_scalar::<_, i64>(
            "SELECT s.from_telegram_id FROM bot_dating_swipes s JOIN bot_dating_profiles p ON p.telegram_id=s.from_telegram_id WHERE s.to_telegram_id=$1 AND s.action='like' AND p.is_active=TRUE AND p.is_paused=FALSE AND p.age_confirmed_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM bot_dating_swipes mutual WHERE mutual.from_telegram_id=$1 AND mutual.to_telegram_id=s.from_telegram_id AND mutual.action='like') AND NOT EXISTS(SELECT 1 FROM bot_dating_blocks b WHERE (b.blocker_telegram_id=$1 AND b.blocked_telegram_id=s.from_telegram_id) OR (b.blocker_telegram_id=s.from_telegram_id AND b.blocked_telegram_id=$1)) ORDER BY s.created_at DESC, s.from_telegram_id DESC LIMIT 1 OFFSET $2"
        )
        .bind(actor)
        .bind(page - 1)
        .fetch_optional(db)
        .await?
    } else { None };
    let next = if page + 1 < total {
        sqlx::query_scalar::<_, i64>(
            "SELECT s.from_telegram_id FROM bot_dating_swipes s JOIN bot_dating_profiles p ON p.telegram_id=s.from_telegram_id WHERE s.to_telegram_id=$1 AND s.action='like' AND p.is_active=TRUE AND p.is_paused=FALSE AND p.age_confirmed_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM bot_dating_swipes mutual WHERE mutual.from_telegram_id=$1 AND mutual.to_telegram_id=s.from_telegram_id AND mutual.action='like') AND NOT EXISTS(SELECT 1 FROM bot_dating_blocks b WHERE (b.blocker_telegram_id=$1 AND b.blocked_telegram_id=s.from_telegram_id) OR (b.blocker_telegram_id=s.from_telegram_id AND b.blocked_telegram_id=$1)) ORDER BY s.created_at DESC, s.from_telegram_id DESC LIMIT 1 OFFSET $2"
        )
        .bind(actor)
        .bind(page + 1)
        .fetch_optional(db)
        .await?
    } else { None };

    let mut rows = vec![vec![button("💚 Ответить лайком", format!("dating:like_incoming:{liker}:{page}"))]];
    let mut navigation = Vec::new();
    if let Some(previous_id) = previous {
        navigation.push(button("◀️ Предыдущий лайк", format!("dating:likeview:{previous_id}:{}", page - 1)));
    }
    if let Some(next_id) = next {
        navigation.push(button("Следующий лайк ▶️", format!("dating:likeview:{next_id}:{}", page + 1)));
    }
    if !navigation.is_empty() { rows.push(navigation); }
    rows.push(nav("dating:likes"));
    rows.push(nav("dating:profile"));
    Ok((text, InlineKeyboardMarkup::new(rows), None))
}

async fn matches_screen(
    db: &PgPool,
    actor: i64,
) -> (String, InlineKeyboardMarkup, Option<(i64, String)>) {
    let rows=sqlx::query_as::<_,(i64,String,String)>("SELECT CASE WHEN m.user1_telegram_id=$1 THEN m.user2_telegram_id ELSE m.user1_telegram_id END,p.display_name,p.bio FROM bot_dating_matches m JOIN bot_dating_profiles p ON p.telegram_id=CASE WHEN m.user1_telegram_id=$1 THEN m.user2_telegram_id ELSE m.user1_telegram_id END WHERE (m.user1_telegram_id=$1 OR m.user2_telegram_id=$1) AND m.status='active' ORDER BY m.created_at DESC LIMIT 20").bind(actor).fetch_all(db).await.unwrap_or_default();
    if rows.is_empty() {
        return ("Пока нет взаимных совпадений. Лайки отправляются приватно и показываются только при взаимном интересе.".into(),InlineKeyboardMarkup::new(vec![vec![button("🔎 Смотреть анкеты","dating:discover")],nav("dating:profile")]),None);
    }
    let mut text =
        String::from("💞 МОИ СОВПАДЕНИЯ\n\nКонтакты открываются только после взаимного лайка.\n");
    let mut keyboard = Vec::new();
    for (id, name, bio) in rows {
        text.push_str(&format!(
            "\n{name} · {}\n",
            if bio.is_empty() {
                "без описания"
            } else {
                &bio
            }
        ));
        keyboard.push(vec![
            contact_button(id),
            button("Закончить общение", format!("dating:unmatch:{id}")),
        ]);
    }
    keyboard.push(nav("dating:profile"));
    (text, InlineKeyboardMarkup::new(keyboard), None)
}

async fn report_queue(db: &PgPool) -> (String, InlineKeyboardMarkup, Option<(i64, String)>) {
    let rows=sqlx::query_as::<_,(i64,i64,i64,String,chrono::DateTime<chrono::Utc>)>("SELECT id,reporter_telegram_id,target_telegram_id,category,created_at FROM bot_dating_reports WHERE status='open' ORDER BY created_at LIMIT 20").fetch_all(db).await.unwrap_or_default();
    if rows.is_empty() {
        return (
            "✅ Открытых жалоб на анкеты нет.".into(),
            InlineKeyboardMarkup::new(vec![nav("ui:main")]),
            None,
        );
    }
    let mut text = String::from("🛡 ЖАЛОБЫ НА АНКЕТЫ\n\n");
    let mut rows_markup = Vec::new();
    for (id, reporter, target, category, created) in rows {
        text.push_str(&format!(
            "#{id} · {category} · анкета {target} · автор {reporter} · {}\n",
            created.format("%d.%m %H:%M")
        ));
        rows_markup.push(vec![
            button(
                format!("#{id} рассмотрена"),
                format!("dating:reportdone:{id}:reviewed"),
            ),
            button("Отклонить", format!("dating:reportdone:{id}:dismissed")),
        ]);
    }
    rows_markup.push(nav("ui:main"));
    (text, InlineKeyboardMarkup::new(rows_markup), None)
}

async fn start_flow(
    db: &PgPool,
    chat_id: i64,
    actor: i64,
    flow: &str,
    message_id: MessageId,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO bot_user_flows(chat_id,telegram_id,flow,step,payload,expires_at) VALUES($1,$2,$3,'text',jsonb_build_object('message_id',$4),now()+interval '10 minutes') ON CONFLICT(chat_id,telegram_id) DO UPDATE SET flow=EXCLUDED.flow,step=EXCLUDED.step,payload=EXCLUDED.payload,expires_at=EXCLUDED.expires_at,updated_at=now()")
        .bind(chat_id).bind(actor).bind(flow).bind(message_id.0).execute(db).await?;
    Ok(())
}

async fn clear_flow(db: &PgPool, chat_id: i64, actor: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM bot_user_flows WHERE chat_id=$1 AND telegram_id=$2 AND flow IN ('cabinet_name','dating_name','dating_bio','dating_photo')").bind(chat_id).bind(actor).execute(db).await?;
    Ok(())
}

pub async fn handle_text_flow(
    bot: &Bot,
    db: &PgPool,
    message: &Message,
    actor: i64,
    text: &str,
) -> SocialResult<bool> {
    if !message.chat.is_private() {
        return Ok(false);
    }
    let flow=sqlx::query_as::<_,(String,serde_json::Value)>("SELECT flow,payload FROM bot_user_flows WHERE chat_id=$1 AND telegram_id=$2 AND flow IN ('cabinet_name','dating_name','dating_bio','dating_photo') AND expires_at>now()")
        .bind(actor).bind(actor).fetch_optional(db).await?;
    let Some((flow, payload)) = flow else {
        return Ok(false);
    };
    let source_id = payload["message_id"].as_i64().unwrap_or_default();

    // Photo upload is accepted only while the user is in the explicit dating_photo flow.
    if flow == "dating_photo" {
        if let Some(photo) = message.photo().and_then(|photos| photos.last()) {
            let saved = sqlx::query(
                "UPDATE bot_dating_profiles SET photo_file_id=$2,updated_at=now() WHERE telegram_id=$1 AND age_confirmed_at IS NOT NULL AND is_active=TRUE"
            )
            .bind(actor)
            .bind(photo.file.id.to_string())
            .execute(db)
            .await?;
            clear_flow(db, actor, actor).await?;
            if saved.rows_affected() == 0 {
                bot.send_message(message.chat.id, "Сначала создай анкету знакомств и подтверди возраст, затем повтори загрузку фото.")
                    .await?;
                return Ok(true);
            }
            bot.send_message(
                message.chat.id,
                "✅ Фото анкеты сохранено. Оно будет использоваться в карточках знакомств. Чтобы заменить фото, снова нажми «Фото анкеты».",
            )
            .reply_markup(InlineKeyboardMarkup::new(vec![
                vec![button("💚 Моя анкета", "dating:profile")],
                vec![button("🗑 Удалить фото", "dating:photo:delete")],
                nav("pc:open"),
            ]))
            .await?;
            return Ok(true);
        }
        if text.trim().eq_ignore_ascii_case("/cancel") {
            clear_flow(db, actor, actor).await?;
            bot.send_message(message.chat.id, "Загрузка фото отменена.")
                .reply_markup(InlineKeyboardMarkup::new(vec![vec![button("💚 Моя анкета", "dating:profile")]]))
                .await?;
        } else {
            bot.send_message(message.chat.id, "Пришли изображение именно как фото, не как файл. Для отмены отправь /cancel.")
                .await?;
        }
        return Ok(true);
    }

    if text.trim().eq_ignore_ascii_case("/cancel") {
        clear_flow(db, actor, actor).await.ok();
        let (screen, kb) = if flow == "cabinet_name" {
            cabinet_screen(db, actor).await
        } else {
            dating_home(db, actor).await
        };
        if source_id > 0 {
            let _ = bot
                .edit_message_text(message.chat.id, MessageId(source_id as i32), screen)
                .reply_markup(kb)
                .await;
        }
        return Ok(true);
    }
    if flow == "dating_photo" {
        bot.send_message(message.chat.id, "Пришли изображение как фото, не как файл. Или нажми /cancel.")
            .await?;
        return Ok(true);
    }
    let value = text.trim();
    let max = if flow == "dating_bio" { 280 } else { 48 };
    if value.chars().count() < 2 || value.chars().count() > max || value.contains('\n') {
        bot.send_message(
            message.chat.id,
            format!("Текст должен быть длиной 2–{max} символов. Повтори или нажми «Отмена»."),
        )
        .await?;
        return Ok(true);
    }
    match flow.as_str() {
        "cabinet_name" => {
            sqlx::query("INSERT INTO bot_user_profiles(telegram_id,display_name) VALUES($1,$2) ON CONFLICT(telegram_id) DO UPDATE SET display_name=EXCLUDED.display_name,updated_at=now()").bind(actor).bind(value).execute(db).await?;
        }
        "dating_name" => {
            sqlx::query("UPDATE bot_dating_profiles SET display_name=$2,updated_at=now() WHERE telegram_id=$1").bind(actor).bind(value).execute(db).await?;
        }
        "dating_bio" => {
            sqlx::query(
                "UPDATE bot_dating_profiles SET bio=$2,updated_at=now() WHERE telegram_id=$1",
            )
            .bind(actor)
            .bind(value)
            .execute(db)
            .await?;
        }
        _ => return Ok(false),
    }
    clear_flow(db, actor, actor).await.ok();
    let (screen, kb) = if flow == "cabinet_name" {
        cabinet_screen(db, actor).await
    } else {
        dating_home(db, actor).await
    };
    if source_id > 0 {
        let _ = bot
            .edit_message_text(message.chat.id, MessageId(source_id as i32), screen)
            .reply_markup(kb)
            .await;
    }
    Ok(true)
}
