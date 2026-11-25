use std::{sync::Arc, time::Duration};
use chrono::{Duration as ChronoDuration, Utc};
use sqlx::PgPool;
use teloxide::{prelude::*, types::Message};

use crate::state::AppState;

fn telegram_user_id(message: &Message) -> Option<i64> {
    message.from.as_ref().and_then(|u| i64::try_from(u.id.0).ok())
}

async fn ensure_user(db: &PgPool, message: &Message) -> Result<i64, sqlx::Error> {
    let telegram_id = telegram_user_id(message).unwrap_or(message.chat.id.0);
    let first_name = message.from.as_ref().map(|u| u.first_name.as_str()).unwrap_or("Студент");
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO users (telegram_id, first_name) VALUES ($1,$2)          ON CONFLICT (telegram_id) DO UPDATE SET first_name=COALESCE(EXCLUDED.first_name,users.first_name), updated_at=now()          RETURNING telegram_id"
    ).bind(telegram_id).bind(first_name).fetch_one(db).await
}

async fn reply(bot: &Bot, db: &PgPool, message: &Message, text: String) -> ResponseResult<()> {
    let sent = bot.send_message(message.chat.id, text).await?;
    let _ = sqlx::query("INSERT INTO bot_messages (telegram_id, chat_id, message_id) VALUES ($1,$2,$3)")
        .bind(telegram_user_id(message).unwrap_or(message.chat.id.0))
        .bind(message.chat.id.0)
        .bind(sent.id.0)
        .execute(db).await;
    Ok(())
}

async fn handle(bot: Bot, state: Arc<AppState>, message: Message) -> ResponseResult<()> {
    let Some(text) = message.text() else { return Ok(()) };
    let db = &state.db;
    let uid = match ensure_user(db, &message).await {
        Ok(id) => id,
        Err(_) => { reply(&bot, db, &message, "Не удалось сохранить профиль. Попробуйте позже.".into()).await?; return Ok(()); }
    };
    let mut parts = text.trim().splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or("").split('@').next().unwrap_or("");
    let argument = parts.next().unwrap_or("").trim();

    let answer = match command {
        "/start" | "/help" => "Привет! Я помощник SMART CAMPUS.\n\n/group название — выбрать группу\n/next — следующая пара\n/today — расписание на сегодня\n/week — ближайшие 7 дней\n/remind_on и /remind_off — напоминания за 10 минут\n/homework текст — добавить задание своей группе\n/clear — удалить мои последние сообщения бота\n/settings — настройки".to_string(),
        "/group" => {
            if argument.is_empty() {
                "Укажи название группы: /group ИВТ-21".to_string()
            } else {
                let group = sqlx::query_as::<_, (uuid::Uuid,String)>(
                    "SELECT id,name FROM groups WHERE is_active=true AND name ILIKE $1 ORDER BY name LIMIT 1"
                ).bind(format!("%{}%", argument)).fetch_optional(db).await;
                match group {
                    Ok(Some((gid,name))) => {
                        let _ = sqlx::query("UPDATE users SET group_id=$2,updated_at=now() WHERE telegram_id=$1").bind(uid).bind(gid).execute(db).await;
                        format!("Группа сохранена: {}. Теперь доступны /next, /today и /week.",name)
                    },
                    Ok(None) => "Не нашёл такую группу. Проверь название или попробуй часть названия.".into(),
                    Err(_) => "Не удалось найти группу.".into()
                }
            }
        },
        "/next" | "/today" | "/week" => {
            let group = sqlx::query_scalar::<_, Option<uuid::Uuid>>("SELECT group_id FROM users WHERE telegram_id=$1").bind(uid).fetch_one(db).await.unwrap_or(None);
            if let Some(gid) = group {
                let now = Utc::now();
                let (from,to) = match command {
                    "/today" => (now.date_naive().and_hms_opt(0,0,0).unwrap().and_utc(), now.date_naive().and_hms_opt(0,0,0).unwrap().and_utc()+ChronoDuration::days(1)),
                    "/week" => (now, now+ChronoDuration::days(7)),
                    _ => (now, now+ChronoDuration::days(14))
                };
                let rows = sqlx::query_as::<_,(String,chrono::DateTime<Utc>,chrono::DateTime<Utc>,Option<String>,Option<String>)>(
                    "SELECT l.subject,l.starts_at,l.ends_at,r.name,t.full_name FROM lessons l LEFT JOIN rooms r ON r.id=l.room_id LEFT JOIN teachers t ON t.id=l.teacher_id WHERE l.group_id=$1 AND l.starts_at >= $2 AND l.starts_at < $3 ORDER BY l.starts_at LIMIT 30"
                ).bind(gid).bind(from).bind(to).fetch_all(db).await.unwrap_or_default();
                if rows.is_empty() { "Пар в выбранном периоде не найдено. Возможно, расписание ещё не загружено.".into() }
                else {
                    let selected = if command=="/next" { rows.into_iter().take(1).collect::<Vec<_>>() } else { rows };
                    selected.into_iter().map(|(subject,start,end,room,teacher)| format!("{}–{} · {}\n{} · {}{}",start.format("%d.%m %H:%M"),end.format("%H:%M"),subject,room.unwrap_or_else(||"аудитория не указана".into()),teacher.unwrap_or_else(||"преподаватель не указан".into()),"")).collect::<Vec<_>>().join("\n\n")
                }
            } else { "Сначала выбери группу командой /group название".into() }
        },
        "/remind_on" | "/remind_off" => {
            let enabled = command=="/remind_on";
            let _ = sqlx::query("UPDATE users SET settings=jsonb_set(settings,'{reminders_enabled}',to_jsonb($2::boolean),true),updated_at=now() WHERE telegram_id=$1").bind(uid).bind(enabled).execute(db).await;
            if enabled { "Напоминания включены: буду писать за 10 минут до пары.".into() } else { "Напоминания выключены.".into() }
        },
        "/settings" => {
            let settings: serde_json::Value = sqlx::query_scalar("SELECT settings FROM users WHERE telegram_id=$1").bind(uid).fetch_one(db).await.unwrap_or_default();
            let group: Option<String> = sqlx::query_scalar("SELECT g.name FROM users u LEFT JOIN groups g ON g.id=u.group_id WHERE u.telegram_id=$1").bind(uid).fetch_one(db).await.unwrap_or(None);
            format!("Текущая группа: {}\nНапоминания: {}\n\nИзменить: /group название, /remind_on, /remind_off",group.unwrap_or_else(||"не выбрана".into()),if settings["reminders_enabled"]==true {"включены"} else {"выключены"})
        },
        "/homework" => {
            if argument.is_empty() { "Добавь текст задания: /homework Подготовить лабораторную №3 к пятнице".into() }
            else {
                let group = sqlx::query_scalar::<_,Option<uuid::Uuid>>("SELECT group_id FROM users WHERE telegram_id=$1").bind(uid).fetch_one(db).await.unwrap_or(None);
                if let Some(gid)=group {
                    let result=sqlx::query("INSERT INTO group_tasks(group_id,title,created_by) SELECT $1,$2,id FROM users WHERE telegram_id=$3")
                        .bind(gid).bind(argument).bind(uid).execute(db).await;
                    if result.is_ok() {"Задание добавлено в список группы. Срок можно будет указать в веб-интерфейсе.".into()} else {"Не удалось добавить задание.".into()}
                } else {"Сначала выбери группу: /group название".into()}
            }
        },
        "/clear" => {
            let ids: Vec<i32> = sqlx::query_scalar("DELETE FROM bot_messages WHERE telegram_id=$1 AND id IN (SELECT id FROM bot_messages WHERE telegram_id=$1 ORDER BY created_at DESC LIMIT 30) RETURNING message_id")
                .bind(uid).fetch_all(db).await.unwrap_or_default();
            let mut removed=0;
            for id in ids { if bot.delete_message(message.chat.id, teloxide::types::MessageId(id)).await.is_ok(){removed+=1;} }
            format!("Удалено сообщений бота: {}. Команду /clear я оставил.",removed)
        },
        _ => return Ok(())
    };
    reply(&bot, db, &message, answer).await?;
    Ok(())
}

pub async fn run(state: Arc<AppState>) {
    let Some(token)=state.config.teloxide_token.clone() else { tracing::warn!("TELOXIDE_TOKEN is not configured; bot is disabled"); return; };
    let bot=Bot::new(token);
    let reminder_bot=bot.clone();
    let reminder_state=state.clone();
    tokio::spawn(async move { reminder_loop(reminder_bot,reminder_state).await; });
    let handler = Update::filter_message().endpoint(move |bot: Bot, message: Message| {
        let state=state.clone();
        async move { handle(bot,state,message).await }
    });
    Dispatcher::builder(bot,handler).build().dispatch().await;
}

async fn reminder_loop(bot: Bot, state: Arc<AppState>) {
    let mut tick=tokio::time::interval(Duration::from_secs(30));
    loop {
        tick.tick().await;
        let rows=sqlx::query_as::<_,(i64,String,String,chrono::DateTime<Utc>)>(
            "SELECT u.telegram_id,l.id::text,l.subject,l.starts_at FROM users u JOIN lessons l ON l.group_id=u.group_id WHERE COALESCE((u.settings->>'reminders_enabled')::boolean,false)=true AND l.starts_at BETWEEN now()+interval '9 minutes' AND now()+interval '10 minutes' AND NOT EXISTS(SELECT 1 FROM bot_reminder_deliveries d WHERE d.telegram_id=u.telegram_id AND d.lesson_id=l.id)"
        ).fetch_all(&state.db).await.unwrap_or_default();
        for (telegram_id, lesson_id, subject, starts_at) in rows {
            let parsed=uuid::Uuid::parse_str(&lesson_id);
            if let Ok(id)=parsed {
                let inserted=sqlx::query("INSERT INTO bot_reminder_deliveries(telegram_id,lesson_id) VALUES($1,$2) ON CONFLICT DO NOTHING")
                    .bind(telegram_id).bind(id).execute(&state.db).await;
                if inserted.is_ok() {
                    let _=bot.send_message(ChatId(telegram_id),format!("⏰ Через 10 минут пара: {} (начало в {}).",subject,starts_at.format("%H:%M"))).await;
                }
            }
        }
    }
}
