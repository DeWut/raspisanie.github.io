use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use sqlx::PgPool;
use teloxide::{prelude::*, types::ChatId};

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
    let is_owner: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND owner_telegram_id=$2 AND is_active=TRUE)"
    ).bind(chat_id).bind(actor).fetch_one(db).await.unwrap_or(false);
    if is_owner { return true; }
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
    if !matches!(command, "/dz" | "/homework" | "/add_dz" | "/edit_dz" | "/delete_dz" | "/done" | "/my_dz" | "/priority_dz") {
        return None;
    }
    let configured: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE)"
    ).bind(chat_id).fetch_one(db).await.unwrap_or(false);
    if !configured {
        return Some("Сначала администратор должен привязать учебную группу: /setgroup название".into());
    }
    if command != "/dz" && command != "/homework" && command != "/done" && !permitted(db, chat_id, actor, "homework").await {
        return Some("⛔ Добавлять и изменять домашние задания в этом чате могут только пользователи с соответствующими правами.".into());
    }
    match command {
        "/my_dz" => {
            let rows = sqlx::query_as::<_, (i64, String, String, Option<DateTime<Utc>>)>(
                "SELECT h.id,h.subject,h.description,h.due_at FROM bot_homework h LEFT JOIN bot_homework_progress p ON p.chat_id=h.chat_id AND p.homework_id=h.id AND p.telegram_id=$2 WHERE h.chat_id=$1 AND h.status='active' AND p.completed_at IS NULL ORDER BY h.due_at NULLS LAST,h.id DESC LIMIT 50"
            ).bind(chat_id).bind(actor).fetch_all(db).await.unwrap_or_default();
            if rows.is_empty() { return Some("🎉 У тебя нет невыполненных заданий в этом чате.".into()); }
            let mut out=String::from("📌 МОИ НЕВЫПОЛНЕННЫЕ ЗАДАНИЯ\n\n");
            for (id,subject,description,due) in rows {
                let due=due.map(|d|d.with_timezone(&moscow()).format("%d.%m %H:%M").to_string()).unwrap_or_else(||"срок не указан".into());
                out.push_str(&format!("#{id} · {subject}\n{description}\nСрок: {due}\n\n"));
            }
            Some(out)
        }
        "/priority_dz" => {
            let p:Vec<_>=argument.split_whitespace().collect();
            if p.len()!=2 || !matches!(p[1],"urgent"|"normal") { return Some("Формат: /priority_dz ID urgent|normal".into()); }
            let Ok(id)=p[0].parse::<i64>() else { return Some("Некорректный ID.".into()); };
            let result=sqlx::query("UPDATE bot_homework SET priority=$3,updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'").bind(chat_id).bind(id).bind(p[1]).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("Приоритет задания #{id} обновлён."),Ok(_)=>"Задание не найдено.".into(),Err(_)=>"Не удалось изменить приоритет.".into() })
        }
        "/dz" | "/homework" => {
            let rows = sqlx::query_as::<_, (i64, String, String, Option<DateTime<Utc>>, String, String)>(
                "SELECT h.id,h.subject,h.description,h.due_at,h.priority,CASE WHEN p.completed_at IS NOT NULL THEN 'completed' ELSE 'not_started' END FROM bot_homework h LEFT JOIN bot_homework_progress p ON p.chat_id=h.chat_id AND p.homework_id=h.id AND p.telegram_id=$2 WHERE h.chat_id=$1 AND h.status='active' ORDER BY h.due_at NULLS LAST,h.id DESC LIMIT 30"
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
            let explicit_due = parts.get(2).copied().unwrap_or("");
            let no_due = matches!(explicit_due.to_lowercase().as_str(), "-" | "нет" | "без срока");
            let due = if no_due { None } else { parse_due(explicit_due) };
            if !explicit_due.is_empty() && !no_due && due.is_none() {
                return Some("Не понял срок. Используй: завтра, послезавтра, ДД.ММ или «без срока».".into());
            }
            let group_id: Option<uuid::Uuid> = sqlx::query_scalar(
                "SELECT group_id FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE"
            ).bind(chat_id).fetch_optional(db).await.ok().flatten();
            // An omitted deadline defaults to the next matching lesson; an explicit
            // "no deadline" value suppresses that automatic fallback.
            let next_lesson = if explicit_due.is_empty() {
                if let Some(group_id) = group_id {
                    sqlx::query_as::<_, (uuid::Uuid, DateTime<Utc>)>(
                        "SELECT id,starts_at FROM lessons WHERE group_id=$1 AND starts_at>now() AND subject ILIKE $2 ORDER BY starts_at LIMIT 1"
                    ).bind(group_id).bind(format!("%{}%",parts[0])).fetch_optional(db).await.ok().flatten()
                } else { None }
            } else { None };
            let deadline = due.or_else(|| next_lesson.as_ref().map(|(_, at)| *at));
            let id: Option<i64> = sqlx::query_scalar(
                "WITH n AS (UPDATE bot_chat_settings SET homework_next_id=homework_next_id+1,updated_at=now() WHERE chat_id=$1 RETURNING homework_next_id-1) INSERT INTO bot_homework(chat_id,id,subject,description,due_at,due_lesson_id,due_lesson_at,created_by) SELECT $1,n.homework_next_id-1,$2,$3,$4,$5,$6,$7 FROM n RETURNING id"
            ).bind(chat_id).bind(parts[0]).bind(parts[1]).bind(deadline)
             .bind(next_lesson.as_ref().map(|(id,_)| *id))
             .bind(next_lesson.as_ref().map(|(_,at)| *at))
             .bind(actor).fetch_optional(db).await.ok().flatten();
            match id {
                Some(id) => Some(format!("✅ Домашнее задание добавлено!\n#{id} · {}\n{}\nСрок: {}",parts[0],parts[1],deadline.map(|d|d.with_timezone(&moscow()).format("%d.%m %H:%M").to_string()).unwrap_or_else(||if no_due {"не указан".into()}else{"не удалось найти следующую пару по этому предмету — укажи срок вручную".into()}))),
                None => Some("Не удалось добавить задание. Проверь, что группа настроена.".into())
            }
        }
        "/edit_dz" => {
            let parts: Vec<_> = argument.splitn(4, '|').map(str::trim).collect();
            if parts.len()<3 { return Some("Формат: /edit_dz ID | предмет | задание | срок".into()); }
            let id=match parts[0].parse::<i64>() { Ok(v) if v>0=>v,_=>return Some("Укажи корректный ID задания.".into()) };
            let due_text=parts.get(3).copied().unwrap_or("");
            let clear_due=matches!(due_text.to_lowercase().as_str(), "-" | "нет" | "без срока");
            let due=if due_text.is_empty() || clear_due {None}else{parse_due(due_text)};
            if !due_text.is_empty() && !clear_due && due.is_none() { return Some("Не понял срок. Используй: завтра, послезавтра, ДД.ММ или «без срока».".into()); }
            let result=sqlx::query("UPDATE bot_homework SET subject=$3,description=$4,due_at=CASE WHEN $6::boolean THEN NULL WHEN $5::timestamptz IS NOT NULL THEN $5 ELSE due_at END,due_lesson_id=CASE WHEN $6::boolean OR $5::timestamptz IS NOT NULL THEN NULL ELSE due_lesson_id END,due_lesson_at=CASE WHEN $6::boolean OR $5::timestamptz IS NOT NULL THEN NULL ELSE due_lesson_at END,updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'")
                .bind(chat_id).bind(id).bind(parts[1]).bind(parts[2]).bind(due).bind(clear_due).execute(db).await;
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


fn parse_moscow_datetime(value: &str) -> Option<DateTime<Utc>> {
    let (date_part, time_part) = value.trim().split_once(' ')?;
    let time = NaiveTime::parse_from_str(time_part.trim(), "%H:%M").ok()?;
    let today = Utc::now().with_timezone(&moscow()).date_naive();
    let date = match date_part.to_lowercase().as_str() {
        "сегодня" => today,
        "завтра" => today.succ_opt()?,
        "послезавтра" => today.checked_add_days(chrono::Days::new(2))?,
        _ => {
            if let Ok(d) = NaiveDate::parse_from_str(date_part, "%d.%m.%Y") { d }
            else {
                let mut p = date_part.split('.');
                let day = p.next()?.parse::<u32>().ok()?;
                let month = p.next()?.parse::<u32>().ok()?;
                if p.next().is_some() { return None; }
                let mut year = today.year();
                let mut d = NaiveDate::from_ymd_opt(year, month, day)?;
                if d < today { year += 1; d = NaiveDate::from_ymd_opt(year, month, day)?; }
                d
            }
        }
    };
    moscow().from_local_datetime(&NaiveDateTime::new(date, time)).single().map(|v| v.with_timezone(&Utc))
}

pub async fn handle_exams_events(bot: &Bot, db: &PgPool, chat_id: i64, actor: i64, command: &str, argument: &str) -> Option<String> {
    let exams = matches!(command, "/exams" | "/add_exam" | "/edit_exam" | "/exam_info" | "/delete_exam");
    let events = matches!(command, "/events" | "/add_event" | "/edit_event" | "/cancel_event" | "/join_event" | "/leave_event" | "/event_capacity" | "/event_waitlist" | "/event_attendees" | "/event_maybe" | "/event_no" | "/event_reminders_on" | "/event_reminders_off");
    if chat_id >= 0 || (!exams && !events) { return None; }
    let configured: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE)")
        .bind(chat_id).fetch_one(db).await.unwrap_or(false);
    if !configured { return Some("Сначала привяжите учебную группу: /setgroup название".into()); }
    let section = if exams { "exams" } else { "events" };
    let writes = !matches!(command, "/exams" | "/events" | "/join_event" | "/leave_event" | "/event_attendees" | "/event_maybe" | "/event_no" | "/event_reminders_on" | "/event_reminders_off");
    if writes && !permitted(db, chat_id, actor, section).await {
        return Some("⛔ Недостаточно прав для управления этим разделом.".into());
    }
    match command {
        "/exams" => {
            let rows = sqlx::query_as::<_, (i64,String,DateTime<Utc>,Option<String>,Option<String>,Option<String>,Option<String>,Option<String>)>(
                "SELECT id,subject,exam_at,room,format,topics,literature,details FROM bot_exams WHERE chat_id=$1 AND status='active' AND exam_at>now()-interval '1 day' ORDER BY exam_at LIMIT 30"
            ).bind(chat_id).fetch_all(db).await.unwrap_or_default();
            if rows.is_empty() { return Some("📝 Экзаменов пока нет. Добавить: /add_exam Предмет | ДД.ММ.ГГГГ ЧЧ:ММ | аудитория | формат | темы | литература | описание".into()); }
            let mut out=String::from("📝 ЭКЗАМЕНЫ\n\n");
            for (id,subject,at,room,format,topics,literature,details) in rows {
                let mut item=format!("#{id} · {subject}\n🕒 {} МСК\n",at.with_timezone(&moscow()).format("%d.%m.%Y %H:%M"));
                for (label,value) in [("Аудитория",room),("Формат",format),("Темы",topics),("Литература",literature),("Описание",details)] {
                    if let Some(value)=value.filter(|v| !v.trim().is_empty()) {
                        let short:String=value.trim().chars().take(180).collect();
                        item.push_str(&format!("{label}: {short}\n"));
                    }
                }
                item.push('\n');
                let remaining=3800usize.saturating_sub(out.chars().count());
                if remaining==0 { break; }
                out.extend(item.chars().take(remaining));
                if out.chars().count()>=3800 { break; }
            }
            Some(out)
        }
        "/add_exam" => {
            let p:Vec<_>=argument.splitn(7,'|').map(str::trim).collect();
            if p.len()<2 || p[0].is_empty() { return Some("Формат: /add_exam Предмет | ДД.ММ.ГГГГ ЧЧ:ММ | аудитория | формат | темы | литература | описание".into()); }
            let Some(at)=parse_moscow_datetime(p[1]) else { return Some("Дата: завтра 09:30 или 15.12.2026 09:30".into()); };
            let field=|index:usize| p.get(index).copied().filter(|v| !v.is_empty() && *v!="-");
            let id:Option<i64>=sqlx::query_scalar("WITH n AS (UPDATE bot_chat_settings SET exam_next_id=exam_next_id+1,updated_at=now() WHERE chat_id=$1 RETURNING exam_next_id-1) INSERT INTO bot_exams(chat_id,id,subject,exam_at,room,format,topics,literature,details,created_by) SELECT $1,n.exam_next_id-1,$2,$3,$4,$5,$6,$7,$8,$9 FROM n RETURNING id")
                .bind(chat_id).bind(p[0]).bind(at).bind(field(2)).bind(field(3)).bind(field(4)).bind(field(5)).bind(field(6)).bind(actor).fetch_optional(db).await.ok().flatten();
            Some(id.map(|v|format!("✅ Экзамен #{v} добавлен.\nПодробности: /exams")).unwrap_or_else(||"Не удалось добавить экзамен.".into()))
        }
        "/edit_exam" => {
            let p:Vec<_>=argument.splitn(8,'|').map(str::trim).collect();
            if p.len()<3 || p[1].is_empty() { return Some("Формат: /edit_exam ID | предмет | дата время | аудитория | формат | темы | литература | описание".into()); }
            let Ok(id)=p[0].parse::<i64>() else { return Some("Некорректный ID.".into()); };
            if id<=0 { return Some("Некорректный ID.".into()); }
            let Some(at)=parse_moscow_datetime(p[2]) else { return Some("Дата: 15.12.2026 09:30".into()); };
            let result=sqlx::query("UPDATE bot_exams SET subject=$3,exam_at=$4,room=CASE WHEN $5::text IS NULL THEN room WHEN $5='-' THEN NULL ELSE NULLIF($5,'') END,format=CASE WHEN $6::text IS NULL THEN format WHEN $6='-' THEN NULL ELSE NULLIF($6,'') END,topics=CASE WHEN $7::text IS NULL THEN topics WHEN $7='-' THEN NULL ELSE NULLIF($7,'') END,literature=CASE WHEN $8::text IS NULL THEN literature WHEN $8='-' THEN NULL ELSE NULLIF($8,'') END,details=CASE WHEN $9::text IS NULL THEN details WHEN $9='-' THEN NULL ELSE NULLIF($9,'') END,updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'")
                .bind(chat_id).bind(id).bind(p[1]).bind(at).bind(p.get(3).copied()).bind(p.get(4).copied()).bind(p.get(5).copied()).bind(p.get(6).copied()).bind(p.get(7).copied()).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("✏️ Экзамен #{id} обновлён."),Ok(_)=>"Экзамен не найден.".into(),Err(_)=>"Ошибка обновления экзамена.".into() })
        }
        "/exam_info" => {
            let p:Vec<_>=argument.splitn(5,'|').map(str::trim).collect();
            if p.len()<2 { return Some("Формат: /exam_info ID | формат | темы | литература | описание (для очистки поля укажи -)".into()); }
            let Ok(id)=p[0].parse::<i64>() else { return Some("Некорректный ID.".into()); };
            if id<=0 { return Some("Некорректный ID.".into()); }
            let result=sqlx::query("UPDATE bot_exams SET format=CASE WHEN $3::text IS NULL THEN format WHEN $3='-' THEN NULL ELSE NULLIF($3,'') END,topics=CASE WHEN $4::text IS NULL THEN topics WHEN $4='-' THEN NULL ELSE NULLIF($4,'') END,literature=CASE WHEN $5::text IS NULL THEN literature WHEN $5='-' THEN NULL ELSE NULLIF($5,'') END,details=CASE WHEN $6::text IS NULL THEN details WHEN $6='-' THEN NULL ELSE NULLIF($6,'') END,updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'")
                .bind(chat_id).bind(id).bind(p.get(1).copied()).bind(p.get(2).copied()).bind(p.get(3).copied()).bind(p.get(4).copied()).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("📘 Подробности экзамена #{id} обновлены."),Ok(_)=>"Экзамен не найден.".into(),Err(_)=>"Не удалось обновить подробности экзамена.".into() })
        }
        "/delete_exam" => {
            let Ok(id)=argument.trim().parse::<i64>() else { return Some("Формат: /delete_exam ID".into()); };
            let result=sqlx::query("UPDATE bot_exams SET status='archived',updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'").bind(chat_id).bind(id).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("🗄 Экзамен #{id} архивирован."),Ok(_)=>"Экзамен не найден.".into(),Err(_)=>"Ошибка удаления экзамена.".into() })
        }
        "/events" => {
            let rows=sqlx::query_as::<_,(i64,String,DateTime<Utc>,Option<String>,Option<String>,Option<i32>,bool,i64,i64)>(
                "SELECT e.id,e.title,e.starts_at,e.location,e.description,e.capacity,e.waitlist_enabled,(SELECT count(*) FROM bot_event_participants p WHERE p.chat_id=e.chat_id AND p.event_id=e.id AND p.response='going'),(SELECT count(*) FROM bot_event_participants p WHERE p.chat_id=e.chat_id AND p.event_id=e.id AND p.response='waitlist') FROM bot_events e WHERE e.chat_id=$1 AND e.status='active' AND e.starts_at>now()-interval '1 day' ORDER BY e.starts_at LIMIT 30"
            ).bind(chat_id).fetch_all(db).await.unwrap_or_default();
            if rows.is_empty() { return Some("🎉 Мероприятий пока нет. Добавить: /add_event Название | ДД.ММ.ГГГГ ЧЧ:ММ | место | описание".into()); }
            let mut out=String::from("🎉 МЕРОПРИЯТИЯ\n\n");
            for (id,title,at,location,description,capacity,waitlist_enabled,going,waiting) in rows {
                let capacity_text=capacity.map(|limit|format!("{going}/{limit} мест занято")).unwrap_or_else(||format!("{going} записались"));
                let queue_text=if waitlist_enabled {format!(" · очередь: {waiting}")} else {" · очередь отключена".into()};
                let mut item=format!("#{id} · {title}\n🕒 {} МСК\n📍 {}\n👥 {capacity_text}{queue_text}\n",at.with_timezone(&moscow()).format("%d.%m.%Y %H:%M"),location.unwrap_or_else(||"Место уточняется".into()));
                if let Some(description)=description.filter(|v|!v.trim().is_empty()) {
                    let short:String=description.trim().chars().take(220).collect();
                    item.push_str(&format!("ℹ️ {short}\n"));
                }
                item.push_str(&format!("/join_event {id} · /leave_event {id}\n\n"));
                let remaining=3800usize.saturating_sub(out.chars().count());
                if remaining==0 { break; }
                out.extend(item.chars().take(remaining));
                if out.chars().count()>=3800 { break; }
            }
            Some(out)
        }
        "/add_event" => {
            let p:Vec<_>=argument.splitn(4,'|').map(str::trim).collect();
            if p.len()<2 || p[0].is_empty() { return Some("Формат: /add_event Название | ДД.ММ.ГГГГ ЧЧ:ММ | место | описание".into()); }
            let Some(at)=parse_moscow_datetime(p[1]) else { return Some("Дата: завтра 18:00 или 20.12.2026 18:00".into()); };
            let field=|index:usize|p.get(index).copied().filter(|v|!v.is_empty()&&*v!="-");
            let id:Option<i64>=sqlx::query_scalar("WITH n AS (UPDATE bot_chat_settings SET event_next_id=event_next_id+1,updated_at=now() WHERE chat_id=$1 RETURNING event_next_id-1) INSERT INTO bot_events(chat_id,id,title,starts_at,location,description,created_by) SELECT $1,n.event_next_id-1,$2,$3,$4,$5,$6 FROM n RETURNING id")
                .bind(chat_id).bind(p[0]).bind(at).bind(field(2)).bind(field(3)).bind(actor).fetch_optional(db).await.ok().flatten();
            Some(id.map(|v|format!("✅ Мероприятие #{v} создано. Запись: /join_event {v}")).unwrap_or_else(||"Не удалось создать мероприятие.".into()))
        }
        "/edit_event" => {
            let p:Vec<_>=argument.splitn(5,'|').map(str::trim).collect();
            if p.len()<3 || p[1].is_empty() { return Some("Формат: /edit_event ID | название | дата время | место | описание".into()); }
            let Ok(id)=p[0].parse::<i64>() else { return Some("Некорректный ID.".into()); };
            if id<=0 { return Some("Некорректный ID.".into()); }
            let Some(at)=parse_moscow_datetime(p[2]) else { return Some("Дата: 20.12.2026 18:00".into()); };
            let mut tx=match db.begin().await {
                Ok(tx)=>tx,
                Err(_)=>return Some("Не удалось начать изменение мероприятия.".into()),
            };
            // Acquire the event lock first, matching join/leave/capacity/cancel lock order.
            let current=match sqlx::query_as::<_,(String,DateTime<Utc>,Option<String>,Option<String>)>(
                "SELECT title,starts_at,location,description FROM bot_events WHERE chat_id=$1 AND id=$2 AND status='active' FOR UPDATE"
            ).bind(chat_id).bind(id).fetch_optional(&mut *tx).await {
                Ok(Some(row))=>row,
                _=>{let _=tx.rollback().await;return Some("Мероприятие не найдено.".into());}
            };
            let recipients=match sqlx::query_scalar::<_,i64>("SELECT telegram_id FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND response IN ('going','maybe','waitlist') FOR UPDATE")
                .bind(chat_id).bind(id).fetch_all(&mut *tx).await {
                Ok(users)=>users,
                Err(_)=>{let _=tx.rollback().await;return Some("Не удалось получить участников мероприятия.".into());}
            };
            let field=|index:usize|p.get(index).copied();
            let updated=sqlx::query("UPDATE bot_events SET title=$3,starts_at=$4,location=CASE WHEN $5::text IS NULL THEN location WHEN $5='-' THEN NULL ELSE NULLIF($5,'') END,description=CASE WHEN $6::text IS NULL THEN description WHEN $6='-' THEN NULL ELSE NULLIF($6,'') END,updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'")
                .bind(chat_id).bind(id).bind(p[1]).bind(at).bind(field(3)).bind(field(4)).execute(&mut *tx).await;
            if !matches!(updated,Ok(ref result) if result.rows_affected()==1) {
                let _=tx.rollback().await;
                return Some("Не удалось обновить мероприятие.".into());
            }
            if tx.commit().await.is_err() {
                return Some("Не удалось сохранить изменение мероприятия.".into());
            }
            let location=match p.get(3).copied() {
                None=>current.2.as_deref().unwrap_or("место уточняется"),
                Some("-")|Some("")=>"место уточняется",
                Some(value)=>value,
            };
            let description=match p.get(4).copied() {
                None=>current.3.as_deref().unwrap_or(""),
                Some("-")|Some("")=>"",
                Some(value)=>value,
            };
            let notice=format!("✏️ Мероприятие #{id} обновлено: {}\n🕒 {} МСК\n📍 {}{}",p[1],at.with_timezone(&moscow()).format("%d.%m.%Y %H:%M"),location,if description.is_empty(){"".into()}else{format!("\nℹ️ {description}")});
            let mut notified=0usize;
            let mut failed=0usize;
            for user in recipients {
                match bot.send_message(ChatId(user),notice.clone()).await {
                    Ok(_)=>notified+=1,
                    Err(error)=>{failed+=1;tracing::debug!("Could not notify event participant {}: {}",user,error);}
                }
            }
            Some(format!("✏️ Мероприятие #{id} обновлено. Личные уведомления: доставлено {notified}, не доставлено {failed}."))
        }
        "/cancel_event" => {
            let Ok(id)=argument.trim().parse::<i64>() else { return Some("Формат: /cancel_event ID".into()); };
            // Signup, leave and capacity changes lock the event row. Locking the
            // same row here serializes cancellation with those operations.
            let mut tx=match db.begin().await {
                Ok(tx)=>tx,
                Err(_)=>return Some("Не удалось начать отмену мероприятия.".into()),
            };
            let event=sqlx::query_scalar::<_,i64>(
                "SELECT id FROM bot_events WHERE chat_id=$1 AND id=$2 AND status='active' FOR UPDATE"
            ).bind(chat_id).bind(id).fetch_optional(&mut *tx).await;
            if !matches!(event,Ok(Some(_))) {
                let _=tx.rollback().await;
                return Some("Мероприятие не найдено или уже отменено.".into());
            }
            let recipients=match sqlx::query_scalar::<_,i64>(
                "SELECT telegram_id FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND response IN ('going','maybe','waitlist') FOR UPDATE"
            ).bind(chat_id).bind(id).fetch_all(&mut *tx).await {
                Ok(users)=>users,
                Err(_)=>{
                    let _=tx.rollback().await;
                    return Some("Не удалось получить список участников мероприятия.".into());
                }
            };
            let updated=sqlx::query(
                "UPDATE bot_events SET status='cancelled',updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'"
            ).bind(chat_id).bind(id).execute(&mut *tx).await;
            if !matches!(updated,Ok(ref result) if result.rows_affected()==1) {
                let _=tx.rollback().await;
                return Some("Не удалось отменить мероприятие.".into());
            }
            if tx.commit().await.is_err() {
                return Some("Не удалось сохранить отмену мероприятия.".into());
            }
            let mut notified=0usize;
            let mut failed=0usize;
            for user in recipients {
                match bot.send_message(ChatId(user),format!("❌ Мероприятие #{id} отменено организатором.")).await {
                    Ok(_)=>notified+=1,
                    Err(error)=>{failed+=1;tracing::debug!("Could not send event cancellation to {}: {}",user,error);}
                }
            }
            Some(format!("❌ Мероприятие #{id} отменено. Личные уведомления: доставлено {notified}, не доставлено {failed}."))
        }
        "/event_attendees" => {
            let Ok(id)=argument.trim().parse::<i64>() else { return Some("Формат: /event_attendees ID".into()); };
            let rows=sqlx::query_as::<_,(i64,String)>("SELECT p.telegram_id, p.response FROM bot_event_participants p JOIN bot_events e ON e.chat_id=p.chat_id AND e.id=p.event_id WHERE p.chat_id=$1 AND p.event_id=$2 AND e.status='active' ORDER BY CASE p.response WHEN 'going' THEN 0 WHEN 'maybe' THEN 1 WHEN 'waitlist' THEN 2 ELSE 3 END,p.joined_at")
                .bind(chat_id).bind(id).fetch_all(db).await.unwrap_or_default();
            if rows.is_empty(){return Some("Пока никто не ответил на это мероприятие.".into());}
            let mut out=format!("👥 УЧАСТНИКИ МЕРОПРИЯТИЯ #{id}\n\n");
            for (user,response) in rows {
                let label=match response.as_str(){"going"=>"Иду","not_going"=>"Не иду","maybe"=>"Думаю","waitlist"=>"Лист ожидания",_=>"Ответ"};
                out.push_str(&format!("• {user} — {label}\n"));
            }
            Some(out)
        }
        "/event_maybe" | "/event_no" => {
            let Ok(id)=argument.trim().parse::<i64>() else { return Some(format!("Формат: {} ID",command)); };
            let response=if command=="/event_maybe" {"maybe"} else {"not_going"};
            let mut tx=match db.begin().await {
                Ok(tx)=>tx,
                Err(_)=>return Some("Не удалось сохранить ответ.".into()),
            };
            let event=sqlx::query_as::<_,(Option<i32>,)>("SELECT capacity FROM bot_events WHERE chat_id=$1 AND id=$2 AND status='active' FOR UPDATE")
                .bind(chat_id).bind(id).fetch_optional(&mut *tx).await;
            if !matches!(event,Ok(Some(_))) {
                let _=tx.rollback().await;
                return Some("Мероприятие не найдено.".into());
            }
            let previous=sqlx::query_scalar::<_,String>("SELECT response FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND telegram_id=$3")
                .bind(chat_id).bind(id).bind(actor).fetch_optional(&mut *tx).await.unwrap_or(None);
            let saved=sqlx::query("INSERT INTO bot_event_participants(chat_id,event_id,telegram_id,response) VALUES($1,$2,$3,$4) ON CONFLICT(chat_id,event_id,telegram_id) DO UPDATE SET response=EXCLUDED.response,updated_at=now()")
                .bind(chat_id).bind(id).bind(actor).bind(response).execute(&mut *tx).await;
            if saved.is_err() {
                let _=tx.rollback().await;
                return Some("Не удалось сохранить ответ.".into());
            }
            let mut promoted_user=None;
            if previous.as_deref()==Some("going") {
                let next=match sqlx::query_scalar::<_,i64>("SELECT telegram_id FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND response='waitlist' ORDER BY joined_at,telegram_id LIMIT 1 FOR UPDATE")
                    .bind(chat_id).bind(id).fetch_optional(&mut *tx).await {
                    Ok(user)=>user,
                    Err(_)=>{
                        let _=tx.rollback().await;
                        return Some("Не удалось обработать лист ожидания.".into());
                    }
                };
                if let Some(user)=next {
                    let promoted=sqlx::query("UPDATE bot_event_participants SET response='going',updated_at=now() WHERE chat_id=$1 AND event_id=$2 AND telegram_id=$3 AND response='waitlist'")
                        .bind(chat_id).bind(id).bind(user).execute(&mut *tx).await;
                    match promoted {
                        Ok(result) if result.rows_affected()==1=>promoted_user=Some(user),
                        _=>{
                            let _=tx.rollback().await;
                            return Some("Не удалось перевести следующего участника из листа ожидания.".into());
                        }
                    }
                }
            }
            if tx.commit().await.is_err() { return Some("Не удалось сохранить ответ.".into()); }
            if let Some(user)=promoted_user {
                if let Err(error)=bot.send_message(ChatId(user),format!("🎉 Освободилось место! Ты записан на мероприятие #{id}.")).await {
                    tracing::debug!("Could not notify promoted event participant {}: {}",user,error);
                }
            }
            let base=if response=="maybe"{format!("🤔 Отмечено: ты думаешь насчёт мероприятия #{id}.")}else{format!("Отмечено: ты не идёшь на мероприятие #{id}.")};
            Some(if promoted_user.is_some() {format!("{base}\nСледующий участник из листа ожидания автоматически добавлен и уведомлён.")} else {base})
        }
        "/event_reminders_on" | "/event_reminders_off" => {
            let Ok(id)=argument.trim().parse::<i64>() else { return Some(format!("Формат: {} ID",command)); };
            let enabled=command=="/event_reminders_on";
            let result=sqlx::query("UPDATE bot_event_participants p SET reminders_enabled=$4,updated_at=now() FROM bot_events e WHERE p.chat_id=$1 AND p.event_id=$2 AND p.telegram_id=$3 AND e.chat_id=p.chat_id AND e.id=p.event_id AND e.status='active'")
                .bind(chat_id).bind(id).bind(actor).bind(enabled).execute(db).await;
            Some(match result {
                Ok(r) if r.rows_affected()==1 => if enabled {format!("🔔 Личные напоминания о мероприятии #{id} включены.")} else {format!("🔕 Личные напоминания о мероприятии #{id} выключены.")},
                Ok(_) => "Сначала запишись на активное мероприятие командой /join_event ID.".into(),
                Err(_) => "Не удалось изменить настройку напоминаний.".into()
            })
        }
        "/event_capacity" => {
            let Some((id_text, capacity_text)) = argument.trim().split_once(' ') else {
                return Some("Формат: /event_capacity ID ЧИСЛО|off".into());
            };
            let Ok(id) = id_text.trim().parse::<i64>() else { return Some("Некорректный ID мероприятия.".into()); };
            let capacity = if capacity_text.trim().eq_ignore_ascii_case("off") { None } else {
                match capacity_text.trim().parse::<i32>() {
                    Ok(v) if (1..=10000).contains(&v) => Some(v),
                    _ => return Some("Вместимость: число от 1 до 10000 или off.".into()),
                }
            };
            let mut tx=match db.begin().await { Ok(tx)=>tx,Err(_)=>return Some("Не удалось изменить вместимость.".into()) };
            let waitlist_enabled=match sqlx::query_scalar::<_,bool>("SELECT waitlist_enabled FROM bot_events WHERE chat_id=$1 AND id=$2 AND status='active' FOR UPDATE")
                .bind(chat_id).bind(id).fetch_optional(&mut *tx).await {
                Ok(Some(enabled))=>enabled,
                _=>{let _=tx.rollback().await;return Some("Мероприятие не найдено.".into());}
            };
            let updated=sqlx::query("UPDATE bot_events SET capacity=$3,updated_at=now() WHERE chat_id=$1 AND id=$2")
                .bind(chat_id).bind(id).bind(capacity).execute(&mut *tx).await;
            if updated.is_err() { let _=tx.rollback().await; return Some("Не удалось изменить вместимость.".into()); }
            let mut demoted:Vec<i64>=Vec::new();
            if let Some(limit)=capacity {
                demoted=match sqlx::query_scalar("WITH ranked AS (SELECT telegram_id,row_number() OVER (ORDER BY joined_at,telegram_id) AS position FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND response='going'), changed AS (UPDATE bot_event_participants p SET response=CASE WHEN $4 THEN 'waitlist' ELSE 'maybe' END,updated_at=now() FROM ranked r WHERE p.chat_id=$1 AND p.event_id=$2 AND p.telegram_id=r.telegram_id AND r.position>$3 RETURNING p.telegram_id) SELECT telegram_id FROM changed")
                    .bind(chat_id).bind(id).bind(i64::from(limit)).bind(waitlist_enabled).fetch_all(&mut *tx).await {
                    Ok(users)=>users,
                    Err(_)=>{let _=tx.rollback().await;return Some("Не удалось согласовать список участников с новой вместимостью.".into());}
                };
            }
            let going:i64=match sqlx::query_scalar("SELECT count(*) FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND response='going'")
                .bind(chat_id).bind(id).fetch_one(&mut *tx).await {
                Ok(count)=>count,
                Err(_)=>{let _=tx.rollback().await;return Some("Не удалось пересчитать участников.".into());}
            };
            let waiting:i64=match sqlx::query_scalar("SELECT count(*) FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND response='waitlist'")
                .bind(chat_id).bind(id).fetch_one(&mut *tx).await {
                Ok(count)=>count,
                Err(_)=>{let _=tx.rollback().await;return Some("Не удалось пересчитать лист ожидания.".into());}
            };
            let slots=if waitlist_enabled { capacity.map(|v|(i64::from(v)-going).max(0)).unwrap_or(waiting) } else {0};
            let promoted:Vec<i64>=match sqlx::query_scalar("WITH chosen AS (SELECT telegram_id FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND response='waitlist' ORDER BY joined_at,telegram_id LIMIT $3 FOR UPDATE), changed AS (UPDATE bot_event_participants p SET response='going',updated_at=now() FROM chosen c WHERE p.chat_id=$1 AND p.event_id=$2 AND p.telegram_id=c.telegram_id RETURNING p.telegram_id) SELECT telegram_id FROM changed")
                .bind(chat_id).bind(id).bind(slots).fetch_all(&mut *tx).await {
                Ok(users)=>users,
                Err(_)=>{let _=tx.rollback().await;return Some("Не удалось обработать лист ожидания.".into());}
            };
            if tx.commit().await.is_err() { return Some("Не удалось сохранить новую вместимость.".into()); }
            for user in &demoted {
                let notice=if waitlist_enabled {
                    format!("ℹ️ Вместимость мероприятия #{id} уменьшена. Ты перемещён в лист ожидания.")
                } else {
                    format!("ℹ️ Вместимость мероприятия #{id} уменьшена. Твоя запись снята, статус изменён на «думаю», так как лист ожидания отключён.")
                };
                if let Err(error)=bot.send_message(ChatId(*user),notice).await {
                    tracing::debug!("Could not notify demoted event participant {}: {}",user,error);
                }
            }
            for user in &promoted {
                if let Err(error)=bot.send_message(ChatId(*user),format!("🎉 Для мероприятия #{id} появилось место! Ты записан.")).await {
                    tracing::debug!("Could not notify promoted event participant {}: {}",user,error);
                }
            }
            Some(format!("✅ Вместимость мероприятия #{id} обновлена. Снято с подтверждённого списка: {}; повышено из очереди: {}.",demoted.len(),promoted.len()))
        }
        "/event_waitlist" => {
            let p:Vec<_>=argument.split_whitespace().collect();
            if p.len()!=2 { return Some("Формат: /event_waitlist ID on|off".into()); }
            let Ok(id)=p[0].parse::<i64>() else { return Some("Некорректный ID мероприятия.".into()); };
            let enabled=match p[1].to_ascii_lowercase().as_str() {
                "on"|"вкл"=>true,
                "off"|"выкл"=>false,
                _=>return Some("Укажи on или off.".into()),
            };
            let mut tx=match db.begin().await { Ok(tx)=>tx,Err(_)=>return Some("Не удалось изменить настройки листа ожидания.".into()) };
            let exists=sqlx::query_scalar::<_,bool>("SELECT waitlist_enabled FROM bot_events WHERE chat_id=$1 AND id=$2 AND status='active' FOR UPDATE")
                .bind(chat_id).bind(id).fetch_optional(&mut *tx).await;
            if !matches!(exists,Ok(Some(_))) { let _=tx.rollback().await; return Some("Мероприятие не найдено.".into()); }
            let waiting:Vec<i64>=if enabled { Vec::new() } else {
                match sqlx::query_scalar("SELECT telegram_id FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND response='waitlist' FOR UPDATE")
                    .bind(chat_id).bind(id).fetch_all(&mut *tx).await {
                    Ok(users)=>users,
                    Err(_)=>{let _=tx.rollback().await;return Some("Не удалось получить очередь участников.".into());}
                }
            };
            let saved=sqlx::query("UPDATE bot_events SET waitlist_enabled=$3,updated_at=now() WHERE chat_id=$1 AND id=$2")
                .bind(chat_id).bind(id).bind(enabled).execute(&mut *tx).await;
            if saved.is_err() { let _=tx.rollback().await;return Some("Не удалось сохранить настройку листа ожидания.".into()); }
            if !enabled {
                let cleared=sqlx::query("UPDATE bot_event_participants SET response='maybe',updated_at=now() WHERE chat_id=$1 AND event_id=$2 AND response='waitlist'")
                    .bind(chat_id).bind(id).execute(&mut *tx).await;
                if cleared.is_err() { let _=tx.rollback().await;return Some("Не удалось очистить лист ожидания.".into()); }
            }
            if tx.commit().await.is_err() { return Some("Не удалось сохранить настройку листа ожидания.".into()); }
            for user in &waiting {
                if let Err(error)=bot.send_message(ChatId(*user),format!("ℹ️ Лист ожидания мероприятия #{id} отключён. Твой статус изменён на «думаю»; при желании ты сможешь снова записаться позже.")).await {
                    tracing::debug!("Could not notify waitlist participant {}: {}",user,error);
                }
            }
            Some(if enabled {
                format!("✅ Лист ожидания мероприятия #{id} включён.")
            } else {
                format!("✅ Лист ожидания мероприятия #{id} отключён. Участников очереди переведено в «думаю»: {}.",waiting.len())
            })
        }
        "/join_event" => {
            let Ok(id)=argument.trim().parse::<i64>() else { return Some("Формат: /join_event ID".into()); };
            let mut tx=match db.begin().await { Ok(tx)=>tx,Err(_)=>return Some("Не удалось начать запись.".into()) };
            let event=sqlx::query_as::<_,(Option<i32>,bool)>("SELECT capacity,waitlist_enabled FROM bot_events WHERE chat_id=$1 AND id=$2 AND status='active' FOR UPDATE")
                .bind(chat_id).bind(id).fetch_optional(&mut *tx).await;
            let Some((capacity,waitlist))=(match event { Ok(v)=>v,Err(_)=>None }) else {
                let _=tx.rollback().await; return Some("Мероприятие не найдено.".into());
            };
            let current=sqlx::query_scalar::<_,String>("SELECT response FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND telegram_id=$3")
                .bind(chat_id).bind(id).bind(actor).fetch_optional(&mut *tx).await.ok().flatten();
            if current.as_deref()==Some("going") { let _=tx.rollback().await; return Some("Ты уже записан на это мероприятие.".into()); }
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND response='going'")
                .bind(chat_id).bind(id).fetch_one(&mut *tx).await.unwrap_or(0);
            let response=if capacity.map(|v|count<i64::from(v)).unwrap_or(true) {"going"} else if waitlist {"waitlist"} else {"maybe"};
            let saved=sqlx::query("INSERT INTO bot_event_participants(chat_id,event_id,telegram_id,response) VALUES($1,$2,$3,$4) ON CONFLICT(chat_id,event_id,telegram_id) DO UPDATE SET response=EXCLUDED.response,updated_at=now()")
                .bind(chat_id).bind(id).bind(actor).bind(response).execute(&mut *tx).await;
            if saved.is_err() || tx.commit().await.is_err() { return Some("Ошибка записи на мероприятие.".into()); }
            Some(match response {
                "going"=>format!("✅ Ты записан на мероприятие #{id}."),
                "waitlist"=>format!("🕒 Мест нет. Ты добавлен в лист ожидания мероприятия #{id}."),
                _=>format!("ℹ️ Мест нет, лист ожидания отключён. Ответ «может быть» сохранён для мероприятия #{id}.")
            })
        }
        "/leave_event" => {
            let Ok(id)=argument.trim().parse::<i64>() else { return Some("Формат: /leave_event ID".into()); };
            let mut tx=match db.begin().await { Ok(tx)=>tx,Err(_)=>return Some("Не удалось отменить запись.".into()) };
            let event_exists=sqlx::query_scalar::<_,i64>("SELECT id FROM bot_events WHERE chat_id=$1 AND id=$2 AND status='active' FOR UPDATE")
                .bind(chat_id).bind(id).fetch_optional(&mut *tx).await;
            if !matches!(event_exists,Ok(Some(_))) { let _=tx.rollback().await; return Some("Мероприятие не найдено или уже отменено.".into()); }
            let removed=sqlx::query("DELETE FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND telegram_id=$3 AND response='going'")
                .bind(chat_id).bind(id).bind(actor).execute(&mut *tx).await;
            let changed=match removed { Ok(r)=>r.rows_affected()==1,Err(_)=>{let _=tx.rollback().await;return Some("Ошибка отмены записи.".into());} };
            let mut promoted_user=None;
            if changed {
                let next=sqlx::query_scalar::<_,i64>("SELECT telegram_id FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND response='waitlist' ORDER BY joined_at,telegram_id LIMIT 1 FOR UPDATE")
                    .bind(chat_id).bind(id).fetch_optional(&mut *tx).await.ok().flatten();
                if let Some(next_id)=next {
                    let promoted=sqlx::query("UPDATE bot_event_participants SET response='going',updated_at=now() WHERE chat_id=$1 AND event_id=$2 AND telegram_id=$3")
                        .bind(chat_id).bind(id).bind(next_id).execute(&mut *tx).await;
                    if matches!(promoted,Ok(r) if r.rows_affected()==1) { promoted_user=Some(next_id); }
                }
            } else {
                let _=sqlx::query("DELETE FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND telegram_id=$3 AND response IN ('waitlist','maybe')")
                    .bind(chat_id).bind(id).bind(actor).execute(&mut *tx).await;
            }
            if tx.commit().await.is_err(){return Some("Не удалось завершить отмену записи.".into());}
            if let Some(user)=promoted_user {
                if let Err(error)=bot.send_message(ChatId(user),format!("🎉 Освободилось место! Ты записан на мероприятие #{id}.")).await {
                    tracing::debug!("Could not notify promoted event participant {}: {}",user,error);
                }
            }
            Some(if changed {
                format!("Запись на мероприятие #{id} отменена. Следующий участник листа ожидания повышен и уведомлён.")
            } else {"Твоя запись или заявка на лист ожидания отменена.".into()})
        }
        _ => None
    }
}


pub async fn handle_content_admin(bot: &Bot, db: &PgPool, chat_id: i64, actor: i64, command: &str, argument: &str) -> Option<String> {
    if chat_id >= 0 || !matches!(command, "/content_rights" | "/content_editor") { return None; }
    let admin: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND telegram_status IN ('creator','administrator')) OR EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND owner_telegram_id=$2 AND is_active=TRUE)")
        .bind(chat_id).bind(actor).fetch_one(db).await.unwrap_or(false);
    if !admin { return Some("⛔ Эти настройки доступны только владельцу SMART CAMPUS, создателю или администраторам Telegram.".into()); }
    match command {
        "/content_rights" => {
            let p:Vec<_>=argument.split_whitespace().collect();
            if p.len()!=2 || !matches!(p[0],"homework"|"exams"|"events") || !matches!(p[1],"everyone"|"admins"|"selected") {
                return Some("Формат: /content_rights homework|exams|events everyone|admins|selected".into());
            }
            let result=sqlx::query("UPDATE bot_chat_settings SET content_permissions=jsonb_set(content_permissions,ARRAY[$2]::text[],to_jsonb($3::text),true),updated_at=now() WHERE chat_id=$1 AND is_active=TRUE")
                .bind(chat_id).bind(p[0]).bind(p[1]).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("✅ Права раздела {}: {}.",p[0],p[1]),Ok(_)=>"Сначала привяжите группу командой /setgroup.".into(),Err(_)=>"Не удалось изменить права.".into() })
        }
        "/content_editor" => {
            let p:Vec<_>=argument.split_whitespace().collect();
            if p.len()!=3 || !matches!(p[0],"homework"|"exams"|"events") || !matches!(p[1],"add"|"remove") {
                return Some("Формат: /content_editor homework|exams|events add|remove TELEGRAM_ID".into());
            }
            let Ok(target)=p[2].parse::<i64>() else { return Some("Укажи числовой Telegram ID участника.".into()); };
            if target<=0 { return Some("Telegram ID должен быть положительным числом.".into()); }
            if p[1]=="add" {
                match crate::services::chat_roles::sync_member(bot, db, ChatId(chat_id), target, None).await {
                    Ok(Some(_)) => {},
                    Ok(None) | Err(_) => return Some("Не удалось подтвердить, что пользователь состоит в этом чате. Попроси участника написать боту в группе и повтори команду.".into()),
                }
            }
            let result=if p[1]=="add" {
                sqlx::query("INSERT INTO bot_chat_content_editors(chat_id,section,telegram_id,assigned_by) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
                    .bind(chat_id).bind(p[0]).bind(target).bind(actor).execute(db).await
            } else {
                sqlx::query("DELETE FROM bot_chat_content_editors WHERE chat_id=$1 AND section=$2 AND telegram_id=$3")
                    .bind(chat_id).bind(p[0]).bind(target).execute(db).await
            };
            Some(match result { Ok(_)=>format!("✅ Права редактора {} для участника {} обновлены.",p[0],target),Err(_)=>"Не удалось обновить список редакторов.".into() })
        }
        _ => None
    }
}


/// Read-only discovery and aggregate statistics for one configured Telegram study chat.
/// All records are scoped by chat_id; personal completion statistics are scoped by actor.
pub async fn handle_group_tools(
    db: &PgPool,
    chat_id: i64,
    actor: i64,
    command: &str,
    argument: &str,
) -> Option<String> {
    if chat_id >= 0 || !matches!(command, "/search" | "/group_stats" | "/my_stats") {
        return None;
    }
    let configured: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE)"
    )
    .bind(chat_id)
    .fetch_one(db)
    .await
    .unwrap_or(false);
    if !configured {
        return Some("Сначала привяжите учебную группу: /setgroup название".into());
    }

    match command {
        "/search" => {
            let query = argument.trim();
            let length = query.chars().count();
            if !(2..=100).contains(&length) {
                return Some("Укажи запрос длиной от 2 до 100 символов: /search линейная алгебра".into());
            }
            let pattern = format!("%{}%", query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            let homework = match sqlx::query_as::<_, (i64,String,String,Option<DateTime<Utc>>)>(
                r#"SELECT id,subject,left(description,180),due_at
                   FROM bot_homework
                   WHERE chat_id=$1 AND status='active'
                     AND (subject ILIKE $2 ESCAPE '\' OR description ILIKE $2 ESCAPE '\')
                   ORDER BY due_at NULLS LAST,id DESC LIMIT 5"#
            )
            .bind(chat_id).bind(&pattern).fetch_all(db).await {
                Ok(rows)=>rows,
                Err(error)=>{tracing::warn!("Group homework search failed: {}",error);return Some("Не удалось выполнить поиск по домашним заданиям.".into());}
            };
            let lessons = match sqlx::query_as::<_, (String,DateTime<Utc>,Option<String>,Option<String>,Option<String>)>(
                r#"SELECT l.subject,l.starts_at,l.lesson_type,t.full_name,r.name
                   FROM bot_chat_settings c
                   JOIN lessons l ON l.group_id=c.group_id
                   LEFT JOIN teachers t ON t.id=l.teacher_id
                   LEFT JOIN rooms r ON r.id=l.room_id
                   WHERE c.chat_id=$1 AND c.is_active=TRUE
                     AND l.starts_at>now()-interval '1 day'
                     AND (l.subject ILIKE $2 ESCAPE '\'
                          OR COALESCE(t.full_name,'') ILIKE $2 ESCAPE '\'
                          OR COALESCE(r.name,'') ILIKE $2 ESCAPE '\')
                   ORDER BY l.starts_at LIMIT 5"#
            )
            .bind(chat_id).bind(&pattern).fetch_all(db).await {
                Ok(rows)=>rows,
                Err(error)=>{tracing::warn!("Group lesson search failed: {}",error);return Some("Не удалось выполнить поиск по расписанию.".into());}
            };
            let exams = match sqlx::query_as::<_, (i64,String,DateTime<Utc>,Option<String>,Option<String>)>(
                r#"SELECT id,subject,exam_at,format,topics
                   FROM bot_exams
                   WHERE chat_id=$1 AND status='active' AND exam_at>now()-interval '1 day'
                     AND (subject ILIKE $2 ESCAPE '\' OR COALESCE(format,'') ILIKE $2 ESCAPE '\'
                          OR COALESCE(topics,'') ILIKE $2 ESCAPE '\' OR COALESCE(details,'') ILIKE $2 ESCAPE '\')
                   ORDER BY exam_at LIMIT 5"#
            )
            .bind(chat_id).bind(&pattern).fetch_all(db).await {
                Ok(rows)=>rows,
                Err(error)=>{tracing::warn!("Group exam search failed: {}",error);return Some("Не удалось выполнить поиск по экзаменам.".into());}
            };
            let events = match sqlx::query_as::<_, (i64,String,DateTime<Utc>,Option<String>)>(
                r#"SELECT id,title,starts_at,location
                   FROM bot_events
                   WHERE chat_id=$1 AND status='active' AND starts_at>now()-interval '1 day'
                     AND (title ILIKE $2 ESCAPE '\' OR COALESCE(description,'') ILIKE $2 ESCAPE '\')
                   ORDER BY starts_at LIMIT 5"#
            )
            .bind(chat_id).bind(&pattern).fetch_all(db).await {
                Ok(rows)=>rows,
                Err(error)=>{tracing::warn!("Group event search failed: {}",error);return Some("Не удалось выполнить поиск по мероприятиям.".into());}
            };
            if homework.is_empty() && lessons.is_empty() && exams.is_empty() && events.is_empty() {
                return Some(format!("По запросу «{query}» ничего не найдено."));
            }
            let mut out=format!("🔎 РЕЗУЛЬТАТЫ ПОИСКА: {query}\n\n");
            if !homework.is_empty() {
                out.push_str("📚 ДОМАШНИЕ ЗАДАНИЯ\n");
                for (id,subject,description,due) in homework {
                    let due=due.map(|v|v.with_timezone(&moscow()).format("%d.%m.%Y %H:%M").to_string()).unwrap_or_else(||"без срока".into());
                    out.push_str(&format!("#{id} · {subject}\n{description}\nСрок: {due}\n\n"));
                }
            }
            if !lessons.is_empty() {
                out.push_str("📅 РАСПИСАНИЕ\n");
                for (subject,at,lesson_type,teacher,room) in lessons {
                    let metadata=vec![
                        lesson_type.map(|v|v.trim().to_owned()).filter(|v|!v.is_empty()),
                        teacher.map(|v|format!("преп. {v}")),
                        room.map(|v|format!("ауд. {v}")),
                    ].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                    out.push_str(&format!("{} · {subject}{}\n\n",
                        at.with_timezone(&moscow()).format("%d.%m.%Y %H:%M"),
                        if metadata.is_empty(){String::new()}else{format!("\n{metadata}")}));
                }
            }
            if !exams.is_empty() {
                out.push_str("📝 ЭКЗАМЕНЫ\n");
                for (id,subject,at,format,topics) in exams {
                    out.push_str(&format!("#{id} · {subject}\n{} МСК{}{}\n\n",
                        at.with_timezone(&moscow()).format("%d.%m.%Y %H:%M"),
                        format.map(|v|format!(" · {v}")).unwrap_or_default(),
                        topics.map(|v|format!("\nТемы: {}",v.chars().take(120).collect::<String>())).unwrap_or_default()));
                }
            }
            if !events.is_empty() {
                out.push_str("🎉 МЕРОПРИЯТИЯ\n");
                for (id,title,at,location) in events {
                    out.push_str(&format!("#{id} · {title}\n{} МСК · {}\n\n",
                        at.with_timezone(&moscow()).format("%d.%m.%Y %H:%M"),
                        location.unwrap_or_else(||"место уточняется".into())));
                }
            }
            if out.chars().count()>3800 {
                out=out.chars().take(3790).collect();
                out.push_str("\n…Результат сокращён.");
            }
            Some(out)
        }
        "/group_stats" => {
            let stats=sqlx::query_as::<_,(i64,i64,i64,i64,i64)>(
                r#"SELECT
                    (SELECT count(*) FROM bot_homework WHERE chat_id=$1 AND status='active'),
                    (SELECT count(*) FROM bot_homework_progress p JOIN bot_homework h ON h.chat_id=p.chat_id AND h.id=p.homework_id WHERE h.chat_id=$1 AND h.status='active'),
                    (SELECT count(*) FROM bot_exams WHERE chat_id=$1 AND status='active' AND exam_at>now()),
                    (SELECT count(*) FROM bot_events WHERE chat_id=$1 AND status='active' AND starts_at>now()),
                    (SELECT count(*) FROM bot_chat_members WHERE chat_id=$1 AND telegram_status IN ('creator','administrator','member'))"#
            )
            .bind(chat_id).fetch_one(db).await;
            let (homework,completed,exams,events,members)=match stats {
                Ok(value)=>value,
                Err(error)=>{tracing::warn!("Group stats query failed: {}",error);return Some("Не удалось получить статистику учебного чата.".into());}
            };
            Some(format!(
                "📊 СТАТИСТИКА УЧЕБНОГО ЧАТА\n\n👥 Активных участников в базе бота: {members}\n📚 Активных домашних заданий: {homework}\n✅ Отметок о выполнении: {completed}\n📝 Предстоящих экзаменов: {exams}\n🎉 Предстоящих мероприятий: {events}"
            ))
        }
        "/my_stats" => {
            let stats=sqlx::query_as::<_,(i64,i64)>(
                r#"SELECT count(*),count(p.completed_at)
                   FROM bot_homework h
                   LEFT JOIN bot_homework_progress p
                     ON p.chat_id=h.chat_id AND p.homework_id=h.id AND p.telegram_id=$2
                   WHERE h.chat_id=$1 AND h.status='active'"#
            )
            .bind(chat_id).bind(actor).fetch_one(db).await;
            let (total,completed)=match stats {
                Ok(value)=>value,
                Err(error)=>{tracing::warn!("Personal homework stats query failed: {}",error);return Some("Не удалось получить твою статистику.".into());}
            };
            let percent=if total==0 {0} else {(completed*100)/total};
            Some(format!(
                "📈 ТВОЯ СТАТИСТИКА ПО ДОМАШНИМ ЗАДАНИЯМ\n\n✅ Выполнено: {completed} из {total}\n📊 Прогресс: {percent}%"
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod group_content_date_tests {
    use super::*;

    #[test]
    fn exam_datetime_parses_tomorrow_in_moscow_time() {
        let expected_date = Utc::now()
            .with_timezone(&moscow())
            .date_naive()
            .succ_opt()
            .expect("tomorrow");
        let parsed = parse_moscow_datetime("завтра 09:30").expect("valid relative date");
        let local = parsed.with_timezone(&moscow());

        assert_eq!(local.date_naive(), expected_date);
        assert_eq!(local.time(), NaiveTime::from_hms_opt(9, 30, 0).unwrap());
    }

    #[test]
    fn exam_datetime_parses_explicit_date_and_time() {
        let today = Utc::now().with_timezone(&moscow()).date_naive();
        let input = format!("{} 14:05", today.format("%d.%m.%Y"));
        let parsed = parse_moscow_datetime(&input).expect("valid explicit date");
        let local = parsed.with_timezone(&moscow());

        assert_eq!(local.date_naive(), today);
        assert_eq!(local.time(), NaiveTime::from_hms_opt(14, 5, 0).unwrap());
    }

    #[test]
    fn exam_datetime_rejects_invalid_date_and_time() {
        assert!(parse_moscow_datetime("32.13.2026 09:30").is_none());
        assert!(parse_moscow_datetime("завтра 25:00").is_none());
        assert!(parse_moscow_datetime("завтра").is_none());
        let single_digit_hour = parse_moscow_datetime("завтра 9:30").expect("single-digit hour is accepted");
        assert_eq!(single_digit_hour.with_timezone(&moscow()).time(), NaiveTime::from_hms_opt(9, 30, 0).unwrap());
    }

    #[test]
    fn homework_due_parser_handles_relative_and_no_deadline_values() {
        assert!(parse_due("завтра").is_some());
        assert!(parse_due("послезавтра").is_some());
        assert!(parse_due("без срока").is_none());
        assert!(parse_due("32.13.2026").is_none());
    }
}
