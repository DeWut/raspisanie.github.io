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
            let due = parse_due(explicit_due);
            if !explicit_due.is_empty() && due.is_none() {
                return Some("Не понял срок. Используй: завтра, послезавтра или ДД.ММ (например, 15.12).".into());
            }
            let group_id: Option<uuid::Uuid> = sqlx::query_scalar(
                "SELECT group_id FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE"
            ).bind(chat_id).fetch_optional(db).await.ok().flatten();
            let next_lesson = if due.is_none() {
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
                Some(id) => Some(format!("✅ Домашнее задание добавлено!\n#{id} · {}\n{}\nСрок: {}",parts[0],parts[1],deadline.map(|d|d.with_timezone(&moscow()).format("%d.%m %H:%M").to_string()).unwrap_or_else(||"не удалось найти следующую пару по этому предмету — укажи срок вручную".into()))),
                None => Some("Не удалось добавить задание. Проверь, что группа настроена.".into())
            }
        }
        "/edit_dz" => {
            let parts: Vec<_> = argument.splitn(4, '|').map(str::trim).collect();
            if parts.len()<3 { return Some("Формат: /edit_dz ID | предмет | задание | срок".into()); }
            let id=match parts[0].parse::<i64>() { Ok(v) if v>0=>v,_=>return Some("Укажи корректный ID задания.".into()) };
            let due_text=parts.get(3).copied().unwrap_or("");
            let due=if due_text.is_empty(){None}else{parse_due(due_text)};
            if !due_text.is_empty() && due.is_none() { return Some("Не понял срок. Используй: завтра, послезавтра или ДД.ММ.".into()); }
            let result=sqlx::query("UPDATE bot_homework SET subject=$3,description=$4,due_at=COALESCE($5,due_at),updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'")
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

pub async fn handle_exams_events(db: &PgPool, chat_id: i64, actor: i64, command: &str, argument: &str) -> Option<String> {
    let exams = matches!(command, "/exams" | "/add_exam" | "/edit_exam" | "/delete_exam");
    let events = matches!(command, "/events" | "/add_event" | "/edit_event" | "/cancel_event" | "/join_event" | "/leave_event" | "/event_capacity" | "/event_attendees" | "/event_maybe" | "/event_no");
    if chat_id >= 0 || (!exams && !events) { return None; }
    let configured: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE)")
        .bind(chat_id).fetch_one(db).await.unwrap_or(false);
    if !configured { return Some("Сначала привяжите учебную группу: /setgroup название".into()); }
    let section = if exams { "exams" } else { "events" };
    let writes = !matches!(command, "/exams" | "/events" | "/join_event" | "/leave_event" | "/event_attendees" | "/event_maybe" | "/event_no");
    if writes && !permitted(db, chat_id, actor, section).await {
        return Some("⛔ Недостаточно прав для управления этим разделом.".into());
    }
    match command {
        "/exams" => {
            let rows = sqlx::query_as::<_, (i64,String,DateTime<Utc>,Option<String>)>(
                "SELECT id,subject,exam_at,room FROM bot_exams WHERE chat_id=$1 AND status='active' AND exam_at>now()-interval '1 day' ORDER BY exam_at LIMIT 30"
            ).bind(chat_id).fetch_all(db).await.unwrap_or_default();
            if rows.is_empty() { return Some("📝 Экзаменов пока нет. Добавить: /add_exam Предмет | ДД.ММ.ГГГГ ЧЧ:ММ | аудитория".into()); }
            let mut out=String::from("📝 ЭКЗАМЕНЫ\n\n");
            for (id,subject,at,room) in rows { out.push_str(&format!("#{id} · {subject}\n🕒 {} МСК\n{}\n\n",at.with_timezone(&moscow()).format("%d.%m.%Y %H:%M"),room.unwrap_or_default())); }
            Some(out)
        }
        "/add_exam" => {
            let p:Vec<_>=argument.splitn(3,'|').map(str::trim).collect();
            if p.len()<2 || p[0].is_empty() { return Some("Формат: /add_exam Предмет | ДД.ММ.ГГГГ ЧЧ:ММ | аудитория".into()); }
            let Some(at)=parse_moscow_datetime(p[1]) else { return Some("Дата: завтра 09:30 или 15.12.2026 09:30".into()); };
            let id:Option<i64>=sqlx::query_scalar("WITH n AS (UPDATE bot_chat_settings SET exam_next_id=exam_next_id+1,updated_at=now() WHERE chat_id=$1 RETURNING exam_next_id-1) INSERT INTO bot_exams(chat_id,id,subject,exam_at,room,created_by) SELECT $1,n.exam_next_id-1,$2,$3,$4,$5 FROM n RETURNING id")
                .bind(chat_id).bind(p[0]).bind(at).bind(p.get(2).copied()).bind(actor).fetch_optional(db).await.ok().flatten();
            Some(id.map(|v|format!("✅ Экзамен #{v} добавлен.")).unwrap_or_else(||"Не удалось добавить экзамен.".into()))
        }
        "/edit_exam" => {
            let p:Vec<_>=argument.splitn(4,'|').map(str::trim).collect();
            if p.len()<3 { return Some("Формат: /edit_exam ID | предмет | дата время | аудитория".into()); }
            let Ok(id)=p[0].parse::<i64>() else { return Some("Некорректный ID.".into()); };
            let Some(at)=parse_moscow_datetime(p[2]) else { return Some("Дата: 15.12.2026 09:30".into()); };
            let result=sqlx::query("UPDATE bot_exams SET subject=$3,exam_at=$4,room=$5,updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'").bind(chat_id).bind(id).bind(p[1]).bind(at).bind(p.get(3).copied()).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("✏️ Экзамен #{id} обновлён."),Ok(_)=>"Экзамен не найден.".into(),Err(_)=>"Ошибка обновления экзамена.".into() })
        }
        "/delete_exam" => {
            let Ok(id)=argument.trim().parse::<i64>() else { return Some("Формат: /delete_exam ID".into()); };
            let result=sqlx::query("UPDATE bot_exams SET status='archived',updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'").bind(chat_id).bind(id).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("🗄 Экзамен #{id} архивирован."),Ok(_)=>"Экзамен не найден.".into(),Err(_)=>"Ошибка удаления экзамена.".into() })
        }
        "/events" => {
            let rows=sqlx::query_as::<_,(i64,String,DateTime<Utc>,Option<String>,i64)>(
                "SELECT e.id,e.title,e.starts_at,e.location,(SELECT count(*) FROM bot_event_participants p WHERE p.chat_id=e.chat_id AND p.event_id=e.id AND p.response='going') FROM bot_events e WHERE e.chat_id=$1 AND e.status='active' AND e.starts_at>now()-interval '1 day' ORDER BY e.starts_at LIMIT 30"
            ).bind(chat_id).fetch_all(db).await.unwrap_or_default();
            if rows.is_empty() { return Some("🎉 Мероприятий пока нет. Добавить: /add_event Название | ДД.ММ.ГГГГ ЧЧ:ММ | место".into()); }
            let mut out=String::from("🎉 МЕРОПРИЯТИЯ\n\n");
            for (id,title,at,location,count) in rows { out.push_str(&format!("#{id} · {title}\n🕒 {} МСК\n📍 {}\n👥 {count} записались\n/join_event {id} · /leave_event {id}\n\n",at.with_timezone(&moscow()).format("%d.%m.%Y %H:%M"),location.unwrap_or_else(||"Место уточняется".into()))); }
            Some(out)
        }
        "/add_event" => {
            let p:Vec<_>=argument.splitn(3,'|').map(str::trim).collect();
            if p.len()<2 || p[0].is_empty() { return Some("Формат: /add_event Название | ДД.ММ.ГГГГ ЧЧ:ММ | место".into()); }
            let Some(at)=parse_moscow_datetime(p[1]) else { return Some("Дата: завтра 18:00 или 20.12.2026 18:00".into()); };
            let id:Option<i64>=sqlx::query_scalar("WITH n AS (UPDATE bot_chat_settings SET event_next_id=event_next_id+1,updated_at=now() WHERE chat_id=$1 RETURNING event_next_id-1) INSERT INTO bot_events(chat_id,id,title,starts_at,location,created_by) SELECT $1,n.event_next_id-1,$2,$3,$4,$5 FROM n RETURNING id")
                .bind(chat_id).bind(p[0]).bind(at).bind(p.get(2).copied()).bind(actor).fetch_optional(db).await.ok().flatten();
            Some(id.map(|v|format!("✅ Мероприятие #{v} создано. Запись: /join_event {v}")).unwrap_or_else(||"Не удалось создать мероприятие.".into()))
        }
        "/edit_event" => {
            let p:Vec<_>=argument.splitn(4,'|').map(str::trim).collect();
            if p.len()<3 { return Some("Формат: /edit_event ID | название | дата время | место".into()); }
            let Ok(id)=p[0].parse::<i64>() else { return Some("Некорректный ID.".into()); };
            let Some(at)=parse_moscow_datetime(p[2]) else { return Some("Дата: 20.12.2026 18:00".into()); };
            let result=sqlx::query("UPDATE bot_events SET title=$3,starts_at=$4,location=$5,updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'").bind(chat_id).bind(id).bind(p[1]).bind(at).bind(p.get(3).copied()).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("✏️ Мероприятие #{id} обновлено."),Ok(_)=>"Мероприятие не найдено.".into(),Err(_)=>"Ошибка обновления мероприятия.".into() })
        }
        "/cancel_event" => {
            let Ok(id)=argument.trim().parse::<i64>() else { return Some("Формат: /cancel_event ID".into()); };
            let result=sqlx::query("UPDATE bot_events SET status='cancelled',updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'").bind(chat_id).bind(id).execute(db).await;
            Some(match result { Ok(r) if r.rows_affected()==1=>format!("❌ Мероприятие #{id} отменено."),Ok(_)=>"Мероприятие не найдено.".into(),Err(_)=>"Ошибка отмены мероприятия.".into() })
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
            let result=sqlx::query("INSERT INTO bot_event_participants(chat_id,event_id,telegram_id,response) SELECT chat_id,id,$3,$4 FROM bot_events WHERE chat_id=$1 AND id=$2 AND status='active' ON CONFLICT(chat_id,event_id,telegram_id) DO UPDATE SET response=EXCLUDED.response,updated_at=now()")
                .bind(chat_id).bind(id).bind(actor).bind(response).execute(db).await;
            Some(match result {
                Ok(r) if r.rows_affected()==1=>if response=="maybe"{format!("🤔 Отмечено: ты думаешь насчёт мероприятия #{id}.")}else{format!("Отмечено: ты не идёшь на мероприятие #{id}.")},
                Ok(_)=>"Мероприятие не найдено.".into(),
                Err(_)=>"Не удалось сохранить ответ.".into()
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
            let result=sqlx::query("UPDATE bot_events SET capacity=$3,updated_at=now() WHERE chat_id=$1 AND id=$2 AND status='active'")
                .bind(chat_id).bind(id).bind(capacity).execute(db).await;
            Some(match result {
                Ok(r) if r.rows_affected()==1 => format!("✅ Вместимость мероприятия #{id} обновлена."),
                Ok(_) => "Мероприятие не найдено.".into(),
                Err(_) => "Не удалось изменить вместимость.".into()
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
            let removed=sqlx::query("DELETE FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND telegram_id=$3 AND response='going'")
                .bind(chat_id).bind(id).bind(actor).execute(&mut *tx).await;
            let changed=match removed { Ok(r)=>r.rows_affected()==1,Err(_)=>{let _=tx.rollback().await;return Some("Ошибка отмены записи.".into());} };
            if changed {
                let next=sqlx::query_scalar::<_,i64>("SELECT telegram_id FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND response='waitlist' ORDER BY joined_at,telegram_id LIMIT 1 FOR UPDATE")
                    .bind(chat_id).bind(id).fetch_optional(&mut *tx).await.ok().flatten();
                if let Some(next_id)=next {
                    let _=sqlx::query("UPDATE bot_event_participants SET response='going',updated_at=now() WHERE chat_id=$1 AND event_id=$2 AND telegram_id=$3")
                        .bind(chat_id).bind(id).bind(next_id).execute(&mut *tx).await;
                }
            } else {
                let _=sqlx::query("DELETE FROM bot_event_participants WHERE chat_id=$1 AND event_id=$2 AND telegram_id=$3 AND response IN ('waitlist','maybe')")
                    .bind(chat_id).bind(id).bind(actor).execute(&mut *tx).await;
            }
            if tx.commit().await.is_err(){return Some("Не удалось завершить отмену записи.".into());}
            Some(if changed {
                format!("Запись на мероприятие #{id} отменена. Первый участник из листа ожидания автоматически повышен, если он был.")
            } else {"Твоя запись или заявка на лист ожидания отменена.".into()})
        }
        _ => None
    }
}


pub async fn handle_content_admin(db: &PgPool, chat_id: i64, actor: i64, command: &str, argument: &str) -> Option<String> {
    if chat_id >= 0 || !matches!(command, "/content_rights" | "/content_editor") { return None; }
    let admin: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND telegram_status IN ('creator','administrator'))")
        .bind(chat_id).bind(actor).fetch_one(db).await.unwrap_or(false);
    if !admin { return Some("⛔ Эти настройки доступны только создателю или администраторам Telegram.".into()); }
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
