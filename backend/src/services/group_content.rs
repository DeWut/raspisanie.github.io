use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use sqlx::PgPool;

fn moscow() -> FixedOffset {
    FixedOffset::east_opt(3 * 60 * 60).expect("UTC+3")
}

fn parse_due(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("нет") || value.eq_ignore_ascii_case("без срока") {
        return None;
    }
    let today = Utc::now().with_timezone(&moscow()).date_naive();
    let date = match value.to_lowercase().as_str() {
        "сегодня" => today,
        "завтра" => today.succ_opt()?,
        "послезавтра" => today.checked_add_days(chrono::Days::new(2))?,
        _ => {
            if let Ok(parsed) = NaiveDate::parse_from_str(value, "%d.%m.%Y") {
                parsed
            } else {
                let mut parts = value.split('.');
                let day = parts.next()?.parse::<u32>().ok()?;
                let month = parts.next()?.parse::<u32>().ok()?;
                if parts.next().is_some() { return None; }
                let mut year = today.year();
                let mut parsed = NaiveDate::from_ymd_opt(year, month, day)?;
                if parsed < today {
                    year += 1;
                    parsed = NaiveDate::from_ymd_opt(year, month, day)?;
                }
                parsed
            }
        }
    };
    let local = NaiveDateTime::new(date, NaiveTime::from_hms_opt(23, 59, 0)?);
    moscow().from_local_datetime(&local).single().map(|v| v.with_timezone(&Utc))
}

use chrono::Datelike;

async fn permitted(db: &PgPool, chat_id: i64, actor: i64, section: &str) -> bool {
    let mode: Option<String> = sqlx::query_scalar(
        "SELECT content_permissions->>$2 FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE"
    ).bind(chat_id).bind(section).fetch_optional(db).await.ok().flatten();
    match mode.as_deref().unwrap_or("everyone") {
        "admins" => sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND telegram_status IN ('creator','administrator'))"
        ).bind(chat_id).bind(actor).fetch_one(db).await.unwrap_or(false),
        "selected" => sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM bot_chat_content_editors WHERE chat_id=$1 AND section=$2 AND telegram_id=$3)"
        ).bind(chat_id).bind(section).bind(actor).fetch_one(db).await.unwrap_or(false),
        _ => true,
    }
}

/// Handles the first group-homework command set. IDs are scoped to a Telegram chat.
pub async fn handle_homework(
    db: &PgPool,
    chat_id: i64,
    actor: i64,
    command: &str,
    argument: &str,
) -> Option<String> {
    if chat_id >= 0 { return None; }
    if !matches!(command, "/dz" | "/add_dz" | "/edit_dz" | "/delete_dz" | "/done") {
        return None;
    }
    let configured: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE)"
    ).bind(chat_id).fetch_one(db).await.unwrap_or(false);
    if !configured {
        return Some("Сначала администратор должен привязать учебную группу: /setgroup название".into());
    }
    if command != "/dz" && !permitted(db, chat_id, actor, "homework").await {
        return Some("⛔ Добавлять и изменять домашние задания в этом чате могут только пользователи с соответствующими правами.".into());
    }
    match command {
        "/dz" => {
            let rows = sqlx::query_as::<_, (i64, String, String, Option<DateTime<Utc>>, String, String)>(
                "SELECT h.id,h.subject,h.description,h.due_at,h.priority,COALESCE(p.status,'not_started') FROM bot_homework h LEFT JOIN bot_homework_progress p ON p.chat_id=h.chat_id AND p.homework_id=h.id AND p.telegram_id=$2 WHERE h.chat_id=$1 AND h.status='active' ORDER BY h.due_at NULLS LAST,h.id DESC LIMIT 30"
            ).bind(chat_id).bind(actor).fetch_all(db).await.unwrap_or_default();
            if rows.is_empty() { return Some("📚 Активных домашних заданий пока нет.\nДобавить: /add_dz Предмет | задание | срок".into()); }
            let mut out = String::from("📚 ДОМАШНИЕ ЗАДАНИЯ\n\n");
            for (id, subject, description, due, priority, status) in rows {
                let due = due.map(|d| d.with_timezone(&moscow()).format("%d.%m %H:%M").to_string()).unwrap_or_else(|| "к следующей паре".into());
                out.push_str(&format!("#{id} · {subject}{}\n{}\nСрок: {due} · {}\n\n",
                    if priority=="urgent" {" 🔥"} else {""}, description,
                    if status=="completed" {"✅ выполнено тобой"} else {"не отмечено"}));
            }
            Some(out)
        }
        "/add_dz" => {
            let parts: Vec<_> = argument.splitn(3, '|').map(str::trim).collect();
            if parts.len()<2 || parts[0].is_empty() || parts[1].is_empty() {
                return Some("Формат: /add_dz Предмет | задание | срок (например, завтра или 15.12)\nСрок можно не указывать.".into());
            }
            let due = parts.get(2).and_then(|s| parse_due(s));
            let id: Option<i64> = sqlx::query_scalar(
                "WITH n AS (UPDATE bot_chat_settings SET homework_next_id=homework_next_id+1,updated_at=now() WHERE chat_id=$1 RETURNING homework_next_id-1) INSERT INTO bot_homework(chat_id,id,subject,description,due_at,created_by) SELECT $1,n.homework_next_id-1,$2,$3,$4,$5 FROM n RETURNING id"
            ).bind(chat_id).bind(parts[0]).bind(parts[1]).bind(due).bind(actor).fetch_optional(db).await.ok().flatten();
            match id {
                Some(id) => Some(format!("✅ Домашнее задание добавлено!\n#{id} · {}\n{}\nСрок: {}",parts[0],parts[1],due.map(|d|d.with_timezone(&moscow()).format("%d.%m %H:%M").to_string()).unwrap_or_else(||"к следующей паре по предмету (автоматический расчёт подключается)".into()))),
                None => Some("Не удалось добавить задание. Проверь, что группа настроена.".into())
            }
        }
        "/edit_dz" => {
            let parts: Vec<_> = argument.splitn(4, '|').map(str::trim).collect();
            if parts.len()<3 { return Some("Формат: /edit_dz ID | предмет | задание | срок".into()); }
            let id=match parts[0].parse::<i64>() { Ok(v) if v>0=>v,_=>return Some("Укажи корректный ID задания.".into()) };
            let due=parts.get(3).and_then(|s|parse_due(s));
            let result=sqlx::query("UPDATE bot_homework SET subject=$3,description=$4,due_at=$5,updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'")
                .bind(chat_id).bind(id).bind(parts[1]).bind(parts[2]).bind(due).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("✏️ Задание #{id} обновлено."),Ok(_)=>"Задание с таким ID не найдено в этом чате.".into(),Err(_)=>"Не удалось изменить задание.".into() })
        }
        "/delete_dz" => {
            let id=match argument.parse::<i64>() { Ok(v) if v>0=>v,_=>return Some("Формат: /delete_dz ID".into()) };
            let result=sqlx::query("UPDATE bot_homework SET status='archived',updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'").bind(chat_id).bind(id).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("🗄 Задание #{id} убрано из активного списка."),Ok(_)=>"Задание с таким ID не найдено.".into(),Err(_)=>"Не удалось удалить задание.".into() })
        }
        "/done" => {
            let id=match argument.parse::<i64>() { Ok(v) if v>0=>v,_=>return Some("Формат: /done ID".into()) };
            let result=sqlx::query("INSERT INTO bot_homework_progress(chat_id,homework_id,telegram_id) SELECT chat_id,id,$3 FROM bot_homework WHERE chat_id=$1 AND id=$2 AND status='active' ON CONFLICT(chat_id,homework_id,telegram_id) DO UPDATE SET completed_at=now()").bind(chat_id).bind(id).bind(actor).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("✅ Задание #{id} отмечено выполненным только для тебя."),Ok(_)=>"Задание не найдено или уже недоступно.".into(),Err(_)=>"Не удалось отметить задание.".into() })
        }
        _ => None
    }
}
