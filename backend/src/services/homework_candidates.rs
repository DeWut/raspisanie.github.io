use chrono::{
    DateTime, Datelike, Days, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc,
};
use sqlx::PgPool;
use teloxide::{
    prelude::*,
    types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup},
};

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: i64,
    pub subject: String,
    pub description: String,
    pub due_at: Option<DateTime<Utc>>,
}

fn moscow() -> FixedOffset {
    FixedOffset::east_opt(3 * 60 * 60).expect("UTC+3")
}

const HOMEWORK_WORDS: &[&str] = &[
    "дз",
    "д/з",
    "домашка",
    "домашнее задание",
    "домашнюю работу",
    "задали",
    "задание на дом",
    "сдать",
    "сдать до",
    "выполнить",
    "сделать",
    "решить",
    "подготовить",
    "написать",
    "прочитать",
    "выучить",
    "принести",
    "лаба",
    "лабу",
    "лабораторная",
    "лабораторную",
    "лабораторку",
    "реферат",
    "доклад",
    "эссе",
    "курсовая",
    "курсовую",
    "проект",
    "презентация",
    "контрольная",
    "самостоятельная",
    "упражнение",
    "задача",
    "задачи",
    "параграф",
    "конспект",
    "отчёт",
    "отчет",
    "тест",
    "семинар",
    "защита",
    "подготовка к",
    "сочинение",
    "перевести",
    "перевод",
    "начертить",
    "нарисовать",
    "оформить",
    "заполнить",
    "повторить",
    "прочесть",
    "таблица",
    "чертёж",
    "чертеж",
    "рассчитать",
    "расчёт",
    "расчет",
    "прорешать",
    "доделать",
    "досдать",
    "к следующей паре",
];

fn has_homework_intent(text: &str) -> bool {
    let lower = text.to_lowercase().replace('ё', "е");
    // Questions and requests to display existing homework must never enter
    // the auto-collection queue as if they were a newly assigned task.
    let is_lookup = [
        "список дз", "список д/з", "список заданий", "покажи дз", "покажи д/з",
        "покажи задания", "какие дз", "какое дз", "какие задания", "какое задание",
        "что задали", "что задано", "что у нас по дз", "напиши список",
        "выведи список", "мои задания", "мое дз", "моё дз", "домашние задания группы",
    ].iter().any(|phrase| lower.contains(phrase));
    !is_lookup && HOMEWORK_WORDS.iter().any(|word| lower.contains(word))
}

fn subject_tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('ё', "е")
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| token.chars().count() >= 3)
        .map(|token| {
            let mut stem = token.to_owned();
            for suffix in [
                "иями", "ями", "ами", "ого", "его", "ому", "ему", "ыми", "ими",
                "ую", "юю", "ая", "яя", "ое", "ее", "ые", "ие", "ый", "ий",
                "ой", "ей", "ам", "ям", "ах", "ях", "ов", "ев", "ом", "ем",
                "а", "я", "ы", "и", "е", "о", "у", "ю",
            ] {
                if stem.ends_with(suffix) && stem.chars().count() > suffix.chars().count() + 2 {
                    stem.truncate(stem.len() - suffix.len());
                    break;
                }
            }
            stem
        })
        .collect()
}

fn resolve_subject(text: &str, aliases: Vec<(String, String)>) -> String {
    let text_lower = text.to_lowercase().replace('ё', "е");
    let text_tokens = subject_tokens(text);
    aliases
        .into_iter()
        .find(|(_, alias)| {
            if alias.trim().is_empty() {
                return false;
            }
            let alias_lower = alias.to_lowercase().replace('ё', "е");
            if text_lower.contains(&alias_lower) {
                return true;
            }
            let alias_tokens = subject_tokens(alias);
            !alias_tokens.is_empty()
                && alias_tokens.iter().all(|token| text_tokens.iter().any(|value| value == token))
        })
        .map(|(subject, _)| subject)
        .unwrap_or_else(|| "Предмет не указан — уточнить".to_owned())
}

fn next_weekday(today: NaiveDate, target: u32) -> Option<NaiveDate> {
    let current = today.weekday().num_days_from_monday();
    let offset = (target + 7 - current) % 7;
    let days = if offset == 0 { 7 } else { offset };
    today.checked_add_days(Days::new(days as u64))
}

fn parse_due(text: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let today = now.with_timezone(&moscow()).date_naive();
    let lower = text.to_lowercase();
    let date = if lower.contains("послезавтра") {
        today.checked_add_days(Days::new(2))
    } else if lower.contains("завтра") {
        today.succ_opt()
    } else if lower.contains("сегодня") {
        Some(today)
    } else if let Some(index) = lower.find("через ") {
        let rest = &lower[index + "через ".len()..];
        let count = rest.split_whitespace().next()?.parse::<i64>().ok()?;
        let days = if rest.contains("недел") {
            count.checked_mul(7)?
        } else {
            count
        };
        (days > 0 && days <= 180)
            .then(|| today.checked_add_signed(Duration::days(days)))
            .flatten()
    } else if let Some(date) = find_date(&lower, today) {
        Some(date)
    } else {
        let weekdays = [
            ("понедельник", 0),
            ("понедельника", 0),
            ("вторник", 1),
            ("вторника", 1),
            ("среду", 2),
            ("среда", 2),
            ("среде", 2),
            ("четверг", 3),
            ("четверга", 3),
            ("пятницу", 4),
            ("пятница", 4),
            ("пятнице", 4),
            ("субботу", 5),
            ("суббота", 5),
            ("воскресенье", 6),
            ("воскресенья", 6),
        ];
        weekdays
            .iter()
            .find(|(name, _)| lower.contains(name))
            .and_then(|(_, day)| next_weekday(today, *day))
    }?;
    moscow()
        .from_local_datetime(&date.and_time(NaiveTime::from_hms_opt(23, 59, 0)?))
        .single()
        .map(|value| value.with_timezone(&Utc))
}

fn find_date(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    for token in text.split(|ch: char| !ch.is_ascii_digit() && ch != '.') {
        let mut parts = token.split('.');
        let Some(day) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(month) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let year = parts.next().and_then(|value| value.parse::<i32>().ok());
        if parts.next().is_some() {
            continue;
        }
        let year = year.unwrap_or_else(|| {
            if NaiveDate::from_ymd_opt(today.year(), month, day).is_some_and(|date| date < today) {
                today.year() + 1
            } else {
                today.year()
            }
        });
        if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
            return Some(date);
        }
    }
    None
}

pub async fn maybe_create(
    db: &PgPool,
    chat_id: i64,
    source_message_id: i32,
    author: i64,
    text: &str,
) -> Result<Option<Candidate>, sqlx::Error> {
    if chat_id >= 0
        || text.trim_start().starts_with('/')
        || text.chars().count() > 4000
        || !has_homework_intent(text)
    {
        return Ok(None);
    }
    let settings = sqlx::query_as::<_, (bool, uuid::Uuid)>(
        "SELECT homework_autocollect_enabled,group_id FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE")
        .bind(chat_id).fetch_optional(db).await?;
    let Some((enabled, group_id)) = settings else {
        return Ok(None);
    };
    if !enabled {
        return Ok(None);
    }

    let aliases = sqlx::query_as::<_, (String, String)>(
        r#"SELECT subject, alias FROM (
               SELECT DISTINCT subject, subject AS alias FROM lessons WHERE group_id=$2
               UNION ALL
               SELECT canonical_subject, alias FROM bot_subject_aliases WHERE chat_id=$1
           ) subjects ORDER BY length(alias) DESC"#,
    )
    .bind(chat_id)
    .bind(group_id)
    .fetch_all(db)
    .await?;
    // A missing exact subject match must not silently discard an otherwise
    // valid homework message. Keep it in the review queue with an explicit
    // placeholder so the approver can correct the subject before publishing.
    let subject = resolve_subject(text, aliases);
    let description: String = text.trim().chars().take(8000).collect();
    let excerpt: String = text.trim().chars().take(1200).collect();
    let normalized = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let source_hash = crate::utils::content_hash(normalized.as_bytes());
    let due_at = parse_due(text, Utc::now());
    let inserted = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO bot_homework_candidates
               (chat_id,source_message_id,source_author_telegram_id,source_hash,subject,description,source_excerpt,due_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
           ON CONFLICT DO NOTHING
           RETURNING id"#)
        .bind(chat_id).bind(source_message_id).bind(author).bind(source_hash).bind(&subject).bind(&description).bind(&excerpt).bind(due_at)
        .fetch_optional(db).await?;
    Ok(inserted.map(|id| Candidate {
        id,
        subject,
        description,
        due_at,
    }))
}

pub fn review_keyboard(id: i64) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("✅ Добавить", format!("hwc:{id}:yes")),
        InlineKeyboardButton::callback("✏️ Изменить", format!("hwc:{id}:edit")),
        InlineKeyboardButton::callback("❌ Пропустить", format!("hwc:{id}:skip")),
    ]])
}

async fn can_review(db: &PgPool, chat_id: i64, actor: i64) -> bool {
    // The chat owner and Telegram administrators retain moderation access
    // regardless of the narrower configured approver role.
    let is_admin: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND owner_telegram_id=$2 AND is_active=TRUE)
           OR EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND telegram_status IN ('creator','administrator'))"#
    )
    .bind(chat_id)
    .bind(actor)
    .fetch_one(db)
    .await
    .unwrap_or(false);
    if is_admin {
        return true;
    }

    let role: Option<String> = sqlx::query_scalar(
        "SELECT homework_approver_role FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE",
    )
    .bind(chat_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten();
    match role.as_deref() {
        Some("class_leader") => sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM bot_chat_member_roles WHERE chat_id=$1 AND telegram_id=$2 AND role='class_leader')")
            .bind(chat_id).bind(actor).fetch_one(db).await.unwrap_or(false),
        Some("moderator") => sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND role IN ('moderator','senior_moderator'))")
            .bind(chat_id).bind(actor).fetch_one(db).await.unwrap_or(false),
        Some("admin") => sqlx::query_scalar::<_, bool>(r#"SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND owner_telegram_id=$2)
             OR EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND telegram_status IN ('creator','administrator'))"#)
            .bind(chat_id).bind(actor).fetch_one(db).await.unwrap_or(false),
        // Older chat settings may have a NULL role. Do not lock everyone out:
        // allow the chat owner, Telegram admins and the designated class leader.
        None => sqlx::query_scalar::<_, bool>(r#"SELECT
             EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND owner_telegram_id=$2 AND is_active=TRUE)
             OR EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND telegram_status IN ('creator','administrator'))
             OR EXISTS(SELECT 1 FROM bot_chat_member_roles WHERE chat_id=$1 AND telegram_id=$2 AND role='class_leader')"#)
            .bind(chat_id).bind(actor).fetch_one(db).await.unwrap_or(false),
        _ => false,
    }
}

pub async fn send_pending_queue(
    bot: &Bot,
    db: &PgPool,
    chat_id: ChatId,
    actor: i64,
) -> Result<(), String> {
    if chat_id.0 >= 0 {
        bot.send_message(chat_id, "Очередь автосбора доступна в учебном групповом чате.")
            .await.map_err(|e| e.to_string())?;
        return Ok(());
    }
    if !can_review(db, chat_id.0, actor).await {
        bot.send_message(chat_id, "⛔ Просматривать и подтверждать собранные задания может выбранная роль подтверждающих.")
            .await.map_err(|e| e.to_string())?;
        return Ok(());
    }

    let enabled: bool = sqlx::query_scalar(
        "SELECT homework_autocollect_enabled FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE"
    ).bind(chat_id.0).fetch_optional(db).await.map_err(|e| e.to_string())?.unwrap_or(false);
    let rows = sqlx::query_as::<_, (i64, String, String, Option<DateTime<Utc>>)>(
        "SELECT id,subject,description,due_at FROM bot_homework_candidates WHERE chat_id=$1 AND status='pending' AND expires_at>now() ORDER BY created_at DESC LIMIT 20"
    ).bind(chat_id.0).fetch_all(db).await.map_err(|e| e.to_string())?;

    bot.send_message(
        chat_id,
        format!("📥 Очередь автосбора ДЗ\nАвтосбор: {}\nОжидают проверки: {}", if enabled { "ВКЛ" } else { "ВЫКЛ" }, rows.len())
    ).await.map_err(|e| e.to_string())?;

    if rows.is_empty() {
        bot.send_message(chat_id, "Новых кандидатов на проверку нет. Когда бот обнаружит задание, оно появится здесь и в сообщении группы.")
            .await.map_err(|e| e.to_string())?;
        return Ok(());
    }

    for (id, subject, description, due_at) in rows {
        let candidate = Candidate { id, subject, description, due_at };
        bot.send_message(chat_id, candidate_text(&candidate))
            .reply_markup(review_keyboard(id))
            .await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn handle_callback(
    bot: &Bot,
    db: &PgPool,
    query: &CallbackQuery,
) -> Result<bool, String> {
    let Some(data) = query.data.as_deref() else {
        return Ok(false);
    };
    let Some(rest) = data.strip_prefix("hwc:") else {
        return Ok(false);
    };
    let Some(message) = query.regular_message() else {
        return Ok(true);
    };
    let chat_id = message.chat.id.0;
    let actor = i64::try_from(query.from.id.0).map_err(|e| e.to_string())?;
    let mut parts = rest.split(':');
    let candidate_id = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or("bad candidate id")?;
    let action = parts.next().unwrap_or("");
    if !can_review(db, chat_id, actor).await {
        let _ = bot
            .answer_callback_query(query.id.clone())
            .text("Это действие доступно только выбранной роли подтверждающих.")
            .show_alert(true)
            .await;
        return Ok(true);
    }
    match action {
        "yes" => {
            let mut tx = db.begin().await.map_err(|e| e.to_string())?;
            let row = sqlx::query_as::<_, (String,String,Option<DateTime<Utc>>)>(
                "SELECT subject,description,due_at FROM bot_homework_candidates WHERE chat_id=$1 AND id=$2 AND status='pending' AND expires_at>now() FOR UPDATE")
                .bind(chat_id).bind(candidate_id).fetch_optional(&mut *tx).await.map_err(|e|e.to_string())?;
            if let Some((subject, description, due_at)) = row {
                let id: i64 = sqlx::query_scalar("UPDATE bot_chat_settings SET homework_next_id=homework_next_id+1,updated_at=now() WHERE chat_id=$1 RETURNING homework_next_id-1")
                    .bind(chat_id).fetch_one(&mut *tx).await.map_err(|e|e.to_string())?;
                sqlx::query("INSERT INTO bot_homework(chat_id,id,subject,description,due_at,created_by) VALUES($1,$2,$3,$4,$5,$6)")
                    .bind(chat_id).bind(id).bind(&subject).bind(&description).bind(due_at).bind(actor).execute(&mut *tx).await.map_err(|e|e.to_string())?;
                sqlx::query("UPDATE bot_homework_candidates SET status='approved',reviewed_by=$3,homework_id=$4 WHERE chat_id=$1 AND id=$2")
                    .bind(chat_id).bind(candidate_id).bind(actor).bind(id).execute(&mut *tx).await.map_err(|e|e.to_string())?;
                tx.commit().await.map_err(|e| e.to_string())?;
                bot.edit_message_text(
                    message.chat.id,
                    message.id,
                    format!("✅ Добавлено в список ДЗ как #{id}: {subject}"),
                )
                .await
                .map_err(|e| e.to_string())?;
            } else {
                let _ = tx.rollback().await;
                bot.edit_message_text(
                    message.chat.id,
                    message.id,
                    "Кандидат уже обработан или срок его хранения истёк.",
                )
                .await
                .map_err(|e| e.to_string())?;
            }
        }
        "skip" => {
            let changed = sqlx::query("UPDATE bot_homework_candidates SET status='rejected',reviewed_by=$3 WHERE chat_id=$1 AND id=$2 AND status='pending'")
                .bind(chat_id).bind(candidate_id).bind(actor).execute(db).await.map_err(|e|e.to_string())?.rows_affected();
            let text = if changed == 1 {
                "❌ Предложение пропущено."
            } else {
                "Кандидат уже обработан."
            };
            bot.edit_message_text(message.chat.id, message.id, text)
                .await
                .map_err(|e| e.to_string())?;
        }
        "edit" => {
            sqlx::query("INSERT INTO bot_user_flows(chat_id,telegram_id,flow,step,payload,expires_at) VALUES($1,$2,'homework_candidate_edit','content',jsonb_build_object('candidate_id',$3),now()+interval '15 minutes') ON CONFLICT(chat_id,telegram_id) DO UPDATE SET flow=EXCLUDED.flow,step=EXCLUDED.step,payload=EXCLUDED.payload,expires_at=EXCLUDED.expires_at,updated_at=now()")
                .bind(chat_id).bind(actor).bind(candidate_id).execute(db).await.map_err(|e|e.to_string())?;
            bot.answer_callback_query(query.id.clone())
                .await
                .map_err(|e| e.to_string())?;
            bot.send_message(message.chat.id,"✏️ Пришли исправление: предмет | описание | срок (ДД.ММ, завтра или без срока).\n/cancel отменит ввод.")
                .await.map_err(|e|e.to_string())?;
            bot.edit_message_reply_markup(message.chat.id, message.id)
                .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                    InlineKeyboardButton::callback(
                        "Ожидает исправления",
                        format!("hwc:{candidate_id}:edit"),
                    ),
                ]]))
                .await
                .map_err(|e| e.to_string())?;
            return Ok(true);
        }
        _ => return Ok(false),
    }
    let _ = sqlx::query("INSERT INTO bot_group_content_audit(chat_id,actor_telegram_id,action,record_type,record_id,details) VALUES($1,$2,$3,'homework',$4,'{}'::jsonb)")
        .bind(chat_id).bind(actor).bind(format!("candidate_{action}" )).bind(candidate_id).execute(db).await;
    let _ = bot.answer_callback_query(query.id.clone()).await;
    Ok(true)
}

pub async fn handle_flow(db: &PgPool, chat_id: i64, actor: i64, text: &str) -> Option<String> {
    let flow = sqlx::query_as::<_, (i64, DateTime<Utc>)>(
        "SELECT (payload->>'candidate_id')::bigint,expires_at FROM bot_user_flows WHERE chat_id=$1 AND telegram_id=$2 AND flow='homework_candidate_edit'")
        .bind(chat_id).bind(actor).fetch_optional(db).await.ok().flatten()?;
    if text.trim().eq_ignore_ascii_case("/cancel") || flow.1 <= Utc::now() {
        let _ = sqlx::query("DELETE FROM bot_user_flows WHERE chat_id=$1 AND telegram_id=$2")
            .bind(chat_id)
            .bind(actor)
            .execute(db)
            .await;
        return Some("Ввод отменён. Кандидат остаётся в очереди.".into());
    }
    let fields = text.splitn(3, '|').map(str::trim).collect::<Vec<_>>();
    if fields.len() < 2
        || fields[0].is_empty()
        || fields[1].is_empty()
        || fields[1].chars().count() > 8000
    {
        return Some(
            "Нужны предмет и описание: предмет | описание | срок. Повтори или отправь /cancel."
                .into(),
        );
    }
    let due_at = fields
        .get(2)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("без срока"))
        .and_then(|value| parse_due(value, Utc::now()));
    let updated = sqlx::query("UPDATE bot_homework_candidates SET subject=$3,description=$4,due_at=$5 WHERE chat_id=$1 AND id=$2 AND status='pending'")
        .bind(chat_id).bind(flow.0).bind(fields[0]).bind(fields[1]).bind(due_at).execute(db).await.ok()?.rows_affected();
    let _ = sqlx::query("DELETE FROM bot_user_flows WHERE chat_id=$1 AND telegram_id=$2")
        .bind(chat_id)
        .bind(actor)
        .execute(db)
        .await;
    Some(if updated == 1 {
        format!("✅ Кандидат #{} исправлен. Староста или выбранная роль может подтвердить его кнопкой в исходном сообщении.",flow.0)
    } else {
        "Кандидат больше не ожидает подтверждения.".into()
    })
}

pub fn candidate_text(candidate: &Candidate) -> String {
    let due = candidate
        .due_at
        .map(|value| {
            value
                .with_timezone(&moscow())
                .format("%d.%m.%Y")
                .to_string()
        })
        .unwrap_or_else(|| "не распознан".into());
    let description: String = candidate.description.chars().take(2500).collect();
    format!("🔎 Возможно, задали ДЗ\n\nПредмет: {}\nОписание: {}\nСрок: {}\n\nЭто только предложение: в общий список попадёт после подтверждения.",candidate.subject,description,due)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_subjects_are_kept_for_manual_review() {
        assert_eq!(
            resolve_subject("ДЗ к пятнице: решить задачи 1–5", vec![]),
            "Предмет не указан — уточнить"
        );
        assert_eq!(
            resolve_subject(
                "По высшей математике решить задачи 1–5",
                vec![("Высшая математика".into(), "Высшая математика".into())]
            ),
            "Высшая математика"
        );
    }

    #[test]
    fn recognizes_homework_without_ai_and_extracts_common_due_dates() {
        let now = moscow()
            .with_ymd_and_hms(2026, 11, 27, 12, 0, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        assert!(has_homework_intent("По физике задали решить задачи 1-5"));
        assert!(!has_homework_intent("Привет, увидимся после пары"));
        assert!(!has_homework_intent("Напиши список дз"));
        assert!(!has_homework_intent("Покажи домашние задания"));
        assert!(!has_homework_intent("Что задали по математике?"));
        assert!(has_homework_intent("ДЗ по математике решить примеры"));
        assert_eq!(
            parse_due("Сдать до 12.12", now)
                .unwrap()
                .with_timezone(&moscow())
                .date_naive(),
            NaiveDate::from_ymd_opt(2026, 12, 12).unwrap()
        );
        assert_eq!(
            parse_due("дз к пятнице", now)
                .unwrap()
                .with_timezone(&moscow())
                .date_naive(),
            NaiveDate::from_ymd_opt(2026, 12, 4).unwrap()
        );
        assert_eq!(
            parse_due("через 2 недели", now)
                .unwrap()
                .with_timezone(&moscow())
                .date_naive(),
            NaiveDate::from_ymd_opt(2026, 12, 11).unwrap()
        );
    }
}
