use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};
use sqlx::PgPool;
use uuid::Uuid;

fn moscow() -> FixedOffset {
    FixedOffset::east_opt(3 * 60 * 60).expect("UTC+3")
}

async fn user_group(db: &PgPool, chat_id: i64, actor: i64) -> Option<Uuid> {
    if chat_id < 0 {
        sqlx::query_scalar(
            "SELECT group_id FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE",
        )
        .bind(chat_id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
    } else {
        sqlx::query_scalar("SELECT group_id FROM users WHERE telegram_id=$1")
            .bind(actor)
            .fetch_one(db)
            .await
            .ok()
            .flatten()
    }
}

fn parse_room_time(value: &str, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let local_now = now.with_timezone(&moscow());
    let today = local_now.date_naive();
    let (date, time): (NaiveDate, NaiveTime) =
        match NaiveTime::parse_from_str(value.trim(), "%H:%M").ok() {
            Some(time) => (today, time),
            None => {
                let current_hour = local_now.time().hour();
                let next_hour = (current_hour + 1) % 24;
                let date = if next_hour == 0 {
                    today.succ_opt().unwrap_or(today)
                } else {
                    today
                };
                (
                    date,
                    NaiveTime::from_hms_opt(next_hour, 0, 0).expect("valid hour"),
                )
            }
        };
    let start = moscow()
        .from_local_datetime(&date.and_time(time))
        .single()
        .unwrap_or_else(|| {
            moscow()
                .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                .single()
                .unwrap()
        })
        .with_timezone(&Utc);
    (start, start + Duration::minutes(90))
}

async fn teacher_schedule(db: &PgPool, query: &str) -> String {
    let query = query.trim();
    if query.chars().count() < 3 || query.chars().count() > 100 {
        return "Укажи фамилию или ФИО преподавателя (от 3 символов): /teacher Иванов".into();
    }
    let pattern = format!(
        "%{}%",
        query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows = sqlx::query_as::<_, (String, String, DateTime<Utc>, Option<String>, String)>(
        r#"SELECT t.full_name, l.subject, l.starts_at, r.name, g.name
           FROM lessons l
           JOIN teachers t ON t.id=l.teacher_id
           JOIN groups g ON g.id=l.group_id
           LEFT JOIN rooms r ON r.id=l.room_id
           WHERE t.full_name ILIKE $1 ESCAPE '\' AND l.starts_at >= now()
           ORDER BY l.starts_at LIMIT 12"#,
    )
    .bind(pattern)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    if rows.is_empty() {
        return format!("Ближайших занятий преподавателя «{query}» не найдено.");
    }
    let mut out = String::from("👨‍🏫 Ближайшие занятия\n\n");
    for (teacher, subject, at, room, group) in rows {
        out.push_str(&format!(
            "{teacher} · {subject}\n{} · {} · {group}\n\n",
            at.with_timezone(&moscow()).format("%d.%m %H:%M"),
            room.unwrap_or_else(|| "аудитория уточняется".into())
        ));
    }
    out
}

async fn free_rooms(db: &PgPool, query: &str) -> String {
    let (from, to) = parse_room_time(query, Utc::now());
    // Room names from EIOS commonly encode the building as the prefix
    // (for example 2.114). Use that when the source has no building label.
    let rows = sqlx::query_as::<_, (String, Option<String>)>(
        r#"SELECT DISTINCT r.name,
                  COALESCE(NULLIF(r.building, ''), substring(r.name from '^([0-9]+)\.')) AS building
           FROM rooms r
           WHERE EXISTS (
               SELECT 1 FROM lessons known
               WHERE known.room_id=r.id
                 AND known.starts_at >= date_trunc('day',$1::timestamptz)
                 AND known.starts_at < date_trunc('day',$1::timestamptz)+interval '1 day'
           )
           AND NOT EXISTS (
               SELECT 1 FROM lessons busy
               WHERE busy.room_id=r.id AND busy.starts_at < $2 AND busy.ends_at > $1
           )
           ORDER BY building NULLS LAST, r.name
           LIMIT 80"#
    )
    .bind(from)
    .bind(to)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    if rows.is_empty() {
        return format!(
            "На {} свободных аудиторий среди известных расписанию не найдено.",
            from.with_timezone(&moscow()).format("%H:%M")
        );
    }

    let mut out = format!(
        "🏫 Свободные аудитории на {} (окно 90 минут)\n\n",
        from.with_timezone(&moscow()).format("%H:%M")
    );
    let mut current_building = String::new();
    for (room, building) in rows {
        let building = building.unwrap_or_else(|| "не определён".into());
        if building != current_building {
            current_building = building.clone();
            out.push_str(&format!("\n🏢 Корпус {building}\n"));
        }
        out.push_str(&format!("• {room}\n"));
    }
    out.push_str("\nПоказаны аудитории, встречающиеся в расписаниях всех групп. Данные обновляются при синхронизации ЕИОС.");
    out
}

async fn what_next(db: &PgPool, group_id: Uuid) -> String {
    let lesson = sqlx::query_as::<_, (String, DateTime<Utc>, Option<String>, Option<String>)>(
        r#"SELECT l.subject,l.starts_at,r.name,t.full_name FROM lessons l
           LEFT JOIN rooms r ON r.id=l.room_id LEFT JOIN teachers t ON t.id=l.teacher_id
           WHERE l.group_id=$1 AND l.starts_at>now() ORDER BY l.starts_at LIMIT 1"#,
    )
    .bind(group_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten();
    let homework = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
        r#"SELECT h.subject,h.due_at FROM bot_homework h
           JOIN bot_chat_settings c ON c.chat_id=h.chat_id
           WHERE c.group_id=$1 AND h.status='active' AND (h.due_at IS NULL OR h.due_at>now())
           ORDER BY h.due_at NULLS LAST LIMIT 3"#,
    )
    .bind(group_id)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    let mut out = String::from("✨ ЧТО ДАЛЬШЕ\n\n");
    match lesson {
        Some((subject, at, room, teacher)) => out.push_str(&format!(
            "⏭ Следующая пара: {subject}\n{} · {}{}\n\n",
            at.with_timezone(&moscow()).format("%d.%m %H:%M"),
            room.unwrap_or_else(|| "аудитория уточняется".into()),
            teacher.map(|v| format!(" · {v}")).unwrap_or_default()
        )),
        None => out.push_str("⏭ Ближайших пар пока нет.\n\n"),
    }
    if homework.is_empty() {
        out.push_str("📚 Активных заданий со сроком нет.");
    } else {
        out.push_str("📚 Ближайшие задания:\n");
        for (subject, due) in homework {
            let due = due
                .map(|v| v.with_timezone(&moscow()).format("%d.%m %H:%M").to_string())
                .unwrap_or_else(|| "срок не указан".into());
            out.push_str(&format!("• {subject} — {due}\n"));
        }
    }
    out
}

async fn group_resources(db: &PgPool, chat_id: i64, query: &str) -> String {
    let pattern = format!("%{}%", query.trim().replace('%', "\\%").replace('_', "\\_"));
    let rows = sqlx::query_as::<_, (i64, Option<String>, String, String)>(
        r#"SELECT id,subject,title,url FROM bot_group_resources
           WHERE chat_id=$1 AND is_active=TRUE AND ($2='' OR title ILIKE $3 OR COALESCE(subject,'') ILIKE $3)
           ORDER BY subject NULLS LAST,created_at DESC LIMIT 30"#,
    ).bind(chat_id).bind(query.trim()).bind(pattern).fetch_all(db).await.unwrap_or_default();
    if rows.is_empty() {
        return "В этом чате пока нет сохранённых материалов. Староста может добавить ссылку: /add_resource Название | URL | предмет".into();
    }
    let mut out = String::from("📎 Материалы группы\n\n");
    for (id, subject, title, url) in rows {
        out.push_str(&format!(
            "#{id} · {}{}\n{url}\n\n",
            title,
            subject.map(|v| format!(" · {v}")).unwrap_or_default()
        ));
    }
    out
}

async fn deadlines(db: &PgPool, chat_id: i64, actor: i64) -> String {
    let Some(group_id) = user_group(db, chat_id, actor).await else {
        return "Сначала выбери группу: /group ИВТ-21".into();
    };
    let group_tasks = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
        "SELECT title,due_at FROM group_tasks WHERE group_id=$1 ORDER BY due_at NULLS LAST,created_at DESC LIMIT 8"
    ).bind(group_id).fetch_all(db).await.unwrap_or_default();
    let chat_tasks = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
        r#"SELECT h.subject,h.due_at FROM bot_homework h
           JOIN bot_chat_settings c ON c.chat_id=h.chat_id
           LEFT JOIN bot_homework_progress p
             ON p.chat_id=h.chat_id AND p.homework_id=h.id AND p.telegram_id=$2
           WHERE c.group_id=$1 AND c.is_active=TRUE AND h.status='active'
             AND p.completed_at IS NULL
             AND (h.due_at IS NULL OR h.due_at>=now())
           ORDER BY h.due_at NULLS LAST LIMIT 8"#,
    )
    .bind(group_id)
    .bind(actor)
    .fetch_all(db)
    .await
    .unwrap_or_default();
    if group_tasks.is_empty() && chat_tasks.is_empty() {
        return "⏰ Ближайших дедлайнов пока нет.".into();
    }
    let mut items = group_tasks
        .into_iter()
        .chain(chat_tasks)
        .collect::<Vec<_>>();
    items.sort_by(|(_, left), (_, right)| match (left, right) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    let mut out = String::from("⏰ Дедлайны группы\n\n");
    for (subject, due) in items.into_iter().take(10) {
        let when = due
            .map(|v| v.with_timezone(&moscow()).format("%d.%m %H:%M").to_string())
            .unwrap_or_else(|| "срок не указан".into());
        out.push_str(&format!("• {subject} — {when}\n"));
    }
    out
}

async fn lost_found_list(db: &PgPool, chat_id: i64) -> String {
    let rows = sqlx::query_as::<_, (i64, String, String, String, i64)>(
        r#"SELECT id,kind,title,details,contact_telegram_id FROM bot_lost_found
           WHERE chat_id=$1 AND status='active' AND expires_at>now()
           ORDER BY created_at DESC LIMIT 12"#,
    )
    .bind(chat_id)
    .fetch_all(db)
    .await
    .unwrap_or_default();
    if rows.is_empty() {
        return "Пока нет объявлений. Создать: /lost Название | детали или /found Название | детали".into();
    }
    let mut out = String::from("🔎 Потеряно / найдено\n\n");
    for (id, kind, title, details, contact) in rows {
        let label = if kind == "lost" {
            "Потеряно"
        } else {
            "Найдено"
        };
        out.push_str(&format!(
            "#{id} · {label}: {title}\n{details}\nСвязаться: tg://user?id={contact}\n\n"
        ));
    }
    out
}

async fn create_lost_found(
    db: &PgPool,
    chat_id: i64,
    actor: i64,
    kind: &str,
    argument: &str,
) -> String {
    if chat_id >= 0 {
        return "Объявления о потерянных вещах публикуются в учебном групповом чате.".into();
    }
    let fields = argument.splitn(2, '|').map(str::trim).collect::<Vec<_>>();
    if fields.len() != 2 || fields[0].chars().count() < 3 || fields[1].chars().count() < 3 {
        return format!("Добавь название и детали: /{kind} Ключи | чёрный брелок, найден у входа");
    }
    if fields[0].chars().count() > 160 || fields[1].chars().count() > 1200 {
        return "Сократи название до 160, а описание до 1200 символов.".into();
    }
    let configured: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE)",
    )
    .bind(chat_id)
    .fetch_one(db)
    .await
    .unwrap_or(false);
    if !configured {
        return "Сначала настройте учебный чат командой /setgroup.".into();
    }
    match sqlx::query_scalar::<_, i64>(
        "INSERT INTO bot_lost_found(chat_id,kind,title,details,contact_telegram_id) VALUES($1,$2,$3,$4,$5) RETURNING id")
        .bind(chat_id).bind(kind).bind(fields[0]).bind(fields[1]).bind(actor).fetch_one(db).await {
        Ok(id) => format!("✅ Объявление #{id} опубликовано на 30 дней. Посмотреть список: /lost_found"),
        Err(_) => "Не удалось сохранить объявление.".into(),
    }
}

async fn add_resource(db: &PgPool, chat_id: i64, actor: i64, argument: &str) -> String {
    let is_editor: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND telegram_status IN ('creator','administrator'))
           OR EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND owner_telegram_id=$2 AND is_active=TRUE)
           OR EXISTS(SELECT 1 FROM bot_chat_member_roles WHERE chat_id=$1 AND telegram_id=$2 AND role='class_leader')"#
    ).bind(chat_id).bind(actor).fetch_one(db).await.unwrap_or(false);
    if !is_editor {
        return "Добавлять ссылки может староста или администратор этого чата.".into();
    }
    let fields = argument.splitn(3, '|').map(str::trim).collect::<Vec<_>>();
    if fields.len() < 2 || fields[0].len() < 2 {
        return "Формат: /add_resource Название | https://ссылка | предмет (необязательно)".into();
    }
    let Ok(url) = url::Url::parse(fields[1]) else {
        return "Нужна полная ссылка, начинающаяся с https://".into();
    };
    if url.scheme() != "https" || url.host_str().is_none() {
        return "Ссылка должна использовать HTTPS.".into();
    }
    let subject = fields.get(2).filter(|v| !v.is_empty()).copied();
    match sqlx::query("INSERT INTO bot_group_resources(chat_id,subject,title,url,added_by) VALUES($1,$2,$3,$4,$5)")
        .bind(chat_id).bind(subject).bind(fields[0]).bind(url.as_str()).bind(actor).execute(db).await {
        Ok(_) => "✅ Ссылка сохранена. Посмотреть её можно в разделе «Материалы».".into(),
        Err(_) => "Не удалось сохранить ссылку.".into(),
    }
}

async fn can_manage_chat(db: &PgPool, chat_id: i64, actor: i64) -> bool {
    sqlx::query_scalar::<_, bool>(
        r#"SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND owner_telegram_id=$2 AND is_active=TRUE)
           OR EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND telegram_status IN ('creator','administrator'))
           OR EXISTS(SELECT 1 FROM bot_chat_member_roles WHERE chat_id=$1 AND telegram_id=$2 AND role='class_leader')"#
    ).bind(chat_id).bind(actor).fetch_one(db).await.unwrap_or(false)
}

async fn can_moderate_reports(db: &PgPool, chat_id: i64, actor: i64) -> bool {
    sqlx::query_scalar::<_, bool>(
        r#"SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND owner_telegram_id=$2 AND is_active=TRUE)
           OR EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND (telegram_status IN ('creator','administrator') OR role IN ('moderator','senior_moderator')))"#
    ).bind(chat_id).bind(actor).fetch_one(db).await.unwrap_or(false)
}

async fn set_autocollect(db: &PgPool, chat_id: i64, actor: i64, enabled: bool) -> String {
    if chat_id >= 0 {
        return "Настройка доступна в учебном групповом чате.".into();
    }
    if !can_manage_chat(db, chat_id, actor).await {
        return "Включить автосбор может староста или администратор чата.".into();
    }
    let result = sqlx::query("UPDATE bot_chat_settings SET homework_autocollect_enabled=$2,updated_at=now() WHERE chat_id=$1 AND is_active=TRUE")
        .bind(chat_id).bind(enabled).execute(db).await;
    if result.is_err() {
        return "Не удалось изменить настройку.".into();
    }
    let _ = sqlx::query("INSERT INTO bot_chat_audit_logs(chat_id,actor_telegram_id,action,details) VALUES($1,$2,'homework_autocollect_toggled',jsonb_build_object('enabled',$3))")
        .bind(chat_id).bind(actor).bind(enabled).execute(db).await;
    if enabled {
        "✅ Автосбор включён. Бот ищет только сообщения с признаками ДЗ и предметом из расписания/словаря; кандидаты ждут подтверждения. Текст хранится только в очереди 7 дней. Для чтения обычных сообщений у бота должен быть доступ к сообщениям группы.".into()
    } else {
        "🔕 Автосбор выключен. Новые сообщения группы не анализируются.".into()
    }
}

pub async fn detect_repeated_spam(
    db: &PgPool,
    chat_id: i64,
    actor: i64,
    message_id: i32,
    text: &str,
) -> bool {
    if chat_id >= 0 || text.trim().chars().count() < 12 {
        return false;
    }
    let enabled: bool = sqlx::query_scalar("SELECT COALESCE((settings->>'spam_filter_enabled')::boolean,FALSE) FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE")
        .bind(chat_id).fetch_optional(db).await.ok().flatten().unwrap_or(false);
    if !enabled {
        return false;
    }
    let normalized = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let hash = crate::utils::content_hash(normalized.as_bytes());
    let count: i32 = match sqlx::query_scalar(
        r#"INSERT INTO bot_chat_spam_seen(chat_id,author_telegram_id,text_hash)
           VALUES($1,$2,$3)
           ON CONFLICT(chat_id,author_telegram_id,text_hash) DO UPDATE SET
             occurrences=CASE WHEN bot_chat_spam_seen.last_seen_at < now()-interval '60 seconds' THEN 1 ELSE bot_chat_spam_seen.occurrences+1 END,
             window_started_at=CASE WHEN bot_chat_spam_seen.last_seen_at < now()-interval '60 seconds' THEN now() ELSE bot_chat_spam_seen.window_started_at END,
             last_seen_at=now()
           RETURNING occurrences"#)
        .bind(chat_id).bind(actor).bind(&hash).fetch_one(db).await {
        Ok(value) => value,
        Err(error) => { tracing::warn!("Spam hash tracking failed: {}", error); return false; }
    };
    if count == 3 {
        let _ = sqlx::query("INSERT INTO bot_chat_reports(chat_id,reporter_telegram_id,target_telegram_id,message_id,category,reason) VALUES($1,0,$2,$3,'spam','Автосигнал: одинаковый текст повторён 3 раза за минуту; исходный текст не сохранён.')")
            .bind(chat_id).bind(actor).bind(message_id).execute(db).await;
        true
    } else {
        false
    }
}

async fn set_spam_filter(db: &PgPool, chat_id: i64, actor: i64, enabled: bool) -> String {
    if chat_id >= 0 {
        return "Фильтр доступен в учебном групповом чате.".into();
    }
    if !can_manage_chat(db, chat_id, actor).await {
        return "Включить фильтр может староста или администратор чата.".into();
    }
    match sqlx::query("UPDATE bot_chat_settings SET settings=jsonb_set(settings,'{spam_filter_enabled}',to_jsonb($2::boolean),true),updated_at=now() WHERE chat_id=$1 AND is_active=TRUE")
        .bind(chat_id).bind(enabled).execute(db).await {
        Ok(_) if enabled => "✅ Фильтр повторов включён. Он сохраняет только хеш текста и создаёт жалобу после трёх одинаковых сообщений за минуту; сообщения не удаляются автоматически.".into(),
        Ok(_) => "🔕 Фильтр повторов выключен.".into(),
        Err(_) => "Не удалось изменить настройку фильтра.".into(),
    }
}

async fn set_approver_role(db: &PgPool, chat_id: i64, actor: i64, role: &str) -> String {
    if chat_id >= 0 {
        return "Настройка доступна в учебном групповом чате.".into();
    }
    if !can_manage_chat(db, chat_id, actor).await {
        return "Изменить подтверждающую роль может староста или администратор чата.".into();
    }
    if !matches!(role, "class_leader" | "moderator" | "admin") {
        return "Допустимые роли: class_leader, moderator, admin. Пример: /autodz_approver moderator".into();
    }
    match sqlx::query("UPDATE bot_chat_settings SET homework_approver_role=$2,updated_at=now() WHERE chat_id=$1 AND is_active=TRUE")
        .bind(chat_id).bind(role).execute(db).await {
        Ok(result) if result.rows_affected()==1 => format!("✅ Найденные задания подтверждает роль: {role}."),
        _ => "Не удалось сохранить роль подтверждающего.".into(),
    }
}

async fn subject_alias(db: &PgPool, chat_id: i64, actor: i64, argument: &str) -> String {
    if chat_id >= 0 {
        return "Словарь настраивается в учебном групповом чате.".into();
    }
    if !can_manage_chat(db, chat_id, actor).await {
        return "Словарь может менять староста или администратор чата.".into();
    }
    let fields = argument.splitn(2, '|').map(str::trim).collect::<Vec<_>>();
    if fields.len() != 2
        || fields
            .iter()
            .any(|v| v.chars().count() < 2 || v.chars().count() > 120)
    {
        return "Формат: /subject_alias Предмет из расписания | сокращение. Например: /subject_alias Высшая математика | вышмат".into();
    }
    match sqlx::query("INSERT INTO bot_subject_aliases(chat_id,canonical_subject,alias,added_by) VALUES($1,$2,$3,$4) ON CONFLICT(chat_id,alias) DO UPDATE SET canonical_subject=EXCLUDED.canonical_subject,added_by=EXCLUDED.added_by")
        .bind(chat_id).bind(fields[0]).bind(fields[1].to_lowercase()).bind(actor).execute(db).await {
        Ok(_) => format!("✅ Добавлено сокращение «{}» → «{}». Доступные предметы: /subject_aliases",fields[1],fields[0]),
        Err(_) => "Не удалось добавить сокращение.".into(),
    }
}

async fn subject_aliases(db: &PgPool, chat_id: i64) -> String {
    if chat_id >= 0 {
        return "Список доступен в учебном групповом чате.".into();
    }
    let rows = sqlx::query_as::<_, (String,String)>("SELECT alias,canonical_subject FROM bot_subject_aliases WHERE chat_id=$1 ORDER BY canonical_subject,alias")
        .bind(chat_id).fetch_all(db).await.unwrap_or_default();
    let mut out = String::from("📖 Сокращения предметов\n\n");
    if rows.is_empty() {
        out.push_str("Пока пусто. Добавить: /subject_alias Предмет | сокращение");
    }
    for (alias, subject) in rows {
        out.push_str(&format!("• {alias} → {subject}\n"));
    }
    out
}

async fn report_queue(db: &PgPool, chat_id: i64, actor: i64) -> String {
    if chat_id >= 0 || !can_moderate_reports(db, chat_id, actor).await {
        return "Очередь жалоб доступна модераторам и администраторам в учебном чате.".into();
    }
    let rows = sqlx::query_as::<_, (i64,Option<i64>,Option<String>,DateTime<Utc>)>(
        "SELECT id,target_telegram_id,reason,created_at FROM bot_chat_reports WHERE chat_id=$1 AND status='open' ORDER BY created_at LIMIT 10")
        .bind(chat_id).fetch_all(db).await.unwrap_or_default();
    if rows.is_empty() {
        return "✅ Открытых жалоб нет.".into();
    }
    let mut out = String::from("🛡 Открытые жалобы\n\n");
    for (id, target, reason, created) in rows {
        out.push_str(&format!(
            "#{id} · участник {} · {}\n{}\nЗакрыть: /report_resolve {id}\n\n",
            target
                .map(|v| v.to_string())
                .unwrap_or_else(|| "не указан".into()),
            created.with_timezone(&moscow()).format("%d.%m %H:%M"),
            reason.unwrap_or_else(|| "без комментария".into())
        ));
    }
    out
}

async fn resolve_report(db: &PgPool, chat_id: i64, actor: i64, argument: &str) -> String {
    if chat_id >= 0 || !can_moderate_reports(db, chat_id, actor).await {
        return "Закрыть жалобу может модератор или администратор.".into();
    }
    let Ok(id) = argument.trim().parse::<i64>() else {
        return "Формат: /report_resolve ID".into();
    };
    match sqlx::query("UPDATE bot_chat_reports SET status='resolved',handled_by=$3,resolved_at=now() WHERE chat_id=$1 AND id=$2 AND status='open'")
        .bind(chat_id).bind(id).bind(actor).execute(db).await {
        Ok(result) if result.rows_affected()==1 => format!("✅ Жалоба #{id} закрыта."),
        Ok(_) => "Открытая жалоба с таким номером не найдена.".into(),
        Err(_) => "Не удалось закрыть жалобу.".into(),
    }
}

async fn report_message(
    db: &PgPool,
    chat_id: i64,
    actor: i64,
    target: Option<i64>,
    message_id: Option<i32>,
    argument: &str,
) -> String {
    if chat_id >= 0 {
        return "Жалоба на сообщение доступна в групповом чате.".into();
    }
    if target == Some(actor) {
        return "Нельзя отправить жалобу на себя.".into();
    }
    if argument.trim().is_empty() && target.is_none() {
        return "Ответь на сообщение и напиши /report причина — содержание сообщения бот не сохраняет.".into();
    }
    let settings_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE)",
    )
    .bind(chat_id)
    .fetch_one(db)
    .await
    .unwrap_or(false);
    if !settings_exists {
        return "Сначала привяжите учебный чат через /setgroup.".into();
    }
    match sqlx::query_scalar::<_, i64>(
        "INSERT INTO bot_chat_reports(chat_id,reporter_telegram_id,target_telegram_id,message_id,reason) VALUES($1,$2,$3,$4,$5) RETURNING id")
        .bind(chat_id).bind(actor).bind(target).bind(message_id).bind(if argument.trim().is_empty(){None}else{Some(argument.trim())})
        .fetch_one(db).await {
        Ok(id) => format!("✅ Жалоба #{id} отправлена модераторам. Текст исходного сообщения не сохранялся."),
        Err(_) => "Не удалось отправить жалобу.".into(),
    }
}

pub async fn handle(
    db: &PgPool,
    chat_id: i64,
    actor: i64,
    command: &str,
    argument: &str,
    reply_target: Option<i64>,
    reply_message_id: Option<i32>,
) -> Option<String> {
    match command {
        "/home" | "/menu" => match user_group(db, chat_id, actor).await {
            Some(group) => Some(what_next(db, group).await),
            None => Some("Сначала выбери учебную группу: /group ИВТ-21. После этого покажу ближайшую пару и задания.".into()),
        },
        "/teacher" | "/teachers" => Some(teacher_schedule(db, argument).await),
        "/free_rooms" => Some(free_rooms(db, argument).await),
        "/deadlines" => Some(deadlines(db, chat_id, actor).await),
        "/resources" => if chat_id < 0 { Some(group_resources(db, chat_id, argument).await) } else { Some("Материалы доступны в учебном чате.".into()) },
        "/add_resource" => if chat_id < 0 { Some(add_resource(db, chat_id, actor, argument).await) } else { Some("Добавлять материалы можно в учебном чате.".into()) },
        "/report" => Some(report_message(db, chat_id, actor, reply_target, reply_message_id, argument).await),
        "/autodz_on" => Some(set_autocollect(db,chat_id,actor,true).await),
        "/autodz_off" => Some(set_autocollect(db,chat_id,actor,false).await),
        "/spam_filter_on" => Some(set_spam_filter(db,chat_id,actor,true).await),
        "/spam_filter_off" => Some(set_spam_filter(db,chat_id,actor,false).await),
        "/autodz_approver" => Some(set_approver_role(db,chat_id,actor,argument).await),
        "/subject_alias" => Some(subject_alias(db,chat_id,actor,argument).await),
        "/subject_aliases" => Some(subject_aliases(db,chat_id).await),
        "/report_queue" => Some(report_queue(db,chat_id,actor).await),
        "/report_resolve" => Some(resolve_report(db,chat_id,actor,argument).await),
        "/lost_found" => if chat_id < 0 { Some(lost_found_list(db, chat_id).await) } else { Some("Поиск объявлений доступен в учебном чате.".into()) },
        "/lost" => Some(create_lost_found(db, chat_id, actor, "lost", argument).await),
        "/found" => Some(create_lost_found(db, chat_id, actor, "found", argument).await),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn room_time_defaults_to_next_hour_and_uses_explicit_time() {
        let now = moscow()
            .with_ymd_and_hms(2026, 11, 27, 10, 25, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let (from, to) = parse_room_time("", now);
        assert_eq!(from.with_timezone(&moscow()).hour(), 11);
        assert_eq!((to - from).num_minutes(), 90);
        let (from, _) = parse_room_time("15:30", now);
        assert_eq!(
            from.with_timezone(&moscow()).time(),
            NaiveTime::from_hms_opt(15, 30, 0).unwrap()
        );

        let late = moscow()
            .with_ymd_and_hms(2026, 11, 27, 23, 30, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let (from, _) = parse_room_time("", late);
        let next_day = from.with_timezone(&moscow());
        assert_eq!(next_day.date_naive(), NaiveDate::from_ymd_opt(2026, 11, 28).unwrap());
        assert_eq!(next_day.time(), NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    }
}
