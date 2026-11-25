use std::collections::HashMap;
use std::time::Duration;

use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, FixedOffset, NaiveDate, NaiveTime, TimeZone,
    Utc,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::models::{GroupSchedule, Lesson};

use super::eios_sync::EiosSync;

const SYNC_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const WEEKS_AHEAD: i64 = 16;
static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
// 23 ноября 2026 года — понедельник нечётной учебной недели.
// При переходе на новый учебный семестр эту дату нужно будет проверить.
const ODD_WEEK_ANCHOR: (i32, u32, u32) = (2026, 11, 23);

const SOURCE: &str = "eios";

/// Запускает синхронизацию сразу, а затем повторяет её каждые 24 часа.
pub async fn run_daily_sync(pool: sqlx::PgPool, eios: EiosSync) {
    tracing::info!("Daily EIOS sync scheduled; first run in 24 hours");

    tokio::time::sleep(SYNC_INTERVAL).await;

    loop {
        tracing::info!("Starting daily EIOS schedule synchronization");

        match sync_all_groups(&pool, &eios).await {
            Ok(()) => {
                tracing::info!("EIOS schedule synchronization finished");
            }
            Err(error) => {
                tracing::error!("EIOS schedule synchronization failed: {}", error);
            }
        }

        tokio::time::sleep(SYNC_INTERVAL).await;
    }
}

pub async fn sync_all_groups(pool: &sqlx::PgPool, eios: &EiosSync) -> Result<(), String> {
    let _guard = SYNC_LOCK
        .try_lock()
        .map_err(|_| "Синхронизация уже выполняется".to_owned())?;

    sync_all_groups_inner(pool, eios).await
}

async fn sync_all_groups_inner(pool: &sqlx::PgPool, eios: &EiosSync) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO sync_metadata (
            source, status, details, last_synced_at, updated_at
        )
        VALUES (
            'eios_schedule', 'running', '{}'::jsonb, now(), now()
        )
        ON CONFLICT (source)
        DO UPDATE SET
            status = 'running',
            details = '{}'::jsonb,
            updated_at = now()
        "#,
    )
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;

    let rows = sqlx::query(
        r#"
        SELECT id, external_id, name
        FROM groups
        WHERE is_active = true
        ORDER BY name
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|error| error.to_string())?;

    let mut successful = 0usize;
    let mut failed = 0usize;
    let mut imported_lessons = 0usize;

    tracing::info!("Active groups found for schedule sync: {}", rows.len());

    for row in rows {
        let group_id: Uuid = row.try_get("id").map_err(|e| e.to_string())?;
        let external_id: String = row.try_get("external_id").map_err(|e| e.to_string())?;
        let group_name: String = row.try_get("name").map_err(|e| e.to_string())?;

        tracing::info!(
            "Downloading schedule for group {} ({})",
            group_name,
            external_id
        );

        let schedule = match eios.sync_group(&external_id).await {
            Ok(schedule) => schedule,
            Err(error) => {
                failed += 1;
                tracing::error!("Could not download schedule for {}: {}", group_name, error);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        let parsed_count = schedule
            .lessons
            .values()
            .flatten()
            .filter(|lesson| is_valid_lesson(lesson))
            .count();

        if parsed_count == 0 {
            failed += 1;
            tracing::warn!(
                "Empty or invalid schedule for {}. Existing database data was preserved.",
                group_name
            );
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }

        match import_group_schedule(pool, group_id, &external_id, &schedule).await {
            Ok(count) => {
                successful += 1;
                imported_lessons += count;

                tracing::info!(
                    "Group {} synchronized: {} lessons imported",
                    group_name,
                    count
                );
            }
            Err(error) => {
                failed += 1;
                tracing::error!("Could not import schedule for {}: {}", group_name, error);
            }
        }

        // Не отправляем запросы к ЭИОС слишком часто.
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    let status = if failed == 0 {
        "success"
    } else if successful == 0 {
        "failed"
    } else {
        "partial"
    };

    let details = json!({
        "groups_total": successful + failed,
        "groups_successful": successful,
        "groups_failed": failed,
        "lessons_imported": imported_lessons,
        "weeks_ahead": WEEKS_AHEAD
    });

    sqlx::query(
        r#"
        UPDATE sync_metadata
        SET
            status = $1,
            details = $2,
            last_synced_at = now(),
            updated_at = now()
        WHERE source = 'eios_schedule'
        "#,
    )
    .bind(status)
    .bind(details)
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;

    tracing::info!(
        "EIOS sync summary: {} successful groups, {} failed groups, {} lessons",
        successful,
        failed,
        imported_lessons
    );

    Ok(())
}

async fn import_group_schedule(
    pool: &sqlx::PgPool,
    group_id: Uuid,
    external_group_id: &str,
    schedule: &GroupSchedule,
) -> Result<usize, String> {
    let offset = FixedOffset::east_opt(3 * 60 * 60)
        .ok_or_else(|| "Invalid Saint Petersburg timezone offset".to_owned())?;

    let today = Utc::now().with_timezone(&offset).date_naive();

    let anchor = NaiveDate::from_ymd_opt(ODD_WEEK_ANCHOR.0, ODD_WEEK_ANCHOR.1, ODD_WEEK_ANCHOR.2)
        .ok_or_else(|| "Invalid academic week anchor".to_owned())?;

    let anchor_monday = anchor
        .checked_sub_signed(ChronoDuration::days(
            anchor.weekday().num_days_from_monday() as i64,
        ))
        .ok_or_else(|| "Could not calculate academic week anchor".to_owned())?;

    let current_monday = today
        .checked_sub_signed(ChronoDuration::days(
            today.weekday().num_days_from_monday() as i64
        ))
        .ok_or_else(|| "Could not calculate current Monday".to_owned())?;

    let end_date = current_monday
        .checked_add_signed(ChronoDuration::weeks(WEEKS_AHEAD))
        .ok_or_else(|| "Could not calculate sync horizon".to_owned())?;

    let start_bound = local_midnight_utc(today, offset)?;
    let end_bound = local_midnight_utc(end_date, offset)?;

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    // Удаляем только будущие занятия, импортированные именно из ЭИОС.
    // Ручные записи и прошедшие занятия не затрагиваются.
    sqlx::query(
        r#"
        DELETE FROM lessons
        WHERE group_id = $1
          AND starts_at >= $2
          AND starts_at < $3
          AND source_data->>'source' = 'eios'
        "#,
    )
    .bind(group_id)
    .bind(start_bound)
    .bind(end_bound)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    let mut teacher_cache = HashMap::<String, Uuid>::new();
    let mut room_cache = HashMap::<String, Uuid>::new();
    let mut imported = 0usize;

    let mut date = today;

    while date < end_date {
        let day_key = date.weekday().number_from_monday().to_string();

        if let Some(day_lessons) = schedule.lessons.get(&day_key) {
            let week_monday = date
                .checked_sub_signed(ChronoDuration::days(
                    date.weekday().num_days_from_monday() as i64
                ))
                .ok_or_else(|| "Could not calculate lesson week".to_owned())?;

            let week_offset = (week_monday - anchor_monday).num_days() / 7;

            let is_odd_week = week_offset.rem_euclid(2) == 0;

            for lesson in day_lessons {
                if !is_valid_lesson(lesson) {
                    continue;
                }

                if !week_matches(&lesson.week, is_odd_week) {
                    continue;
                }

                let start_time = NaiveTime::parse_from_str(&lesson.start, "%H:%M")
                    .map_err(|e| format!("Invalid lesson start time: {e}"))?;

                let end_time = NaiveTime::parse_from_str(&lesson.end, "%H:%M")
                    .map_err(|e| format!("Invalid lesson end time: {e}"))?;

                let starts_at = local_datetime_utc(date, start_time, offset)?;

                let ends_at = local_datetime_utc(date, end_time, offset)?;

                if ends_at <= starts_at {
                    continue;
                }

                let teacher_id = if lesson.teacher.trim().is_empty() {
                    None
                } else if let Some(id) = teacher_cache.get(&lesson.teacher) {
                    Some(*id)
                } else {
                    let id = upsert_teacher(&mut tx, &lesson.teacher).await?;
                    teacher_cache.insert(lesson.teacher.clone(), id);
                    Some(id)
                };

                let room_id = if lesson.room.trim().is_empty() {
                    None
                } else if let Some(id) = room_cache.get(&lesson.room) {
                    Some(*id)
                } else {
                    let id = upsert_room(&mut tx, &lesson.room).await?;
                    room_cache.insert(lesson.room.clone(), id);
                    Some(id)
                };

                let external_lesson_id = stable_id(
                    "lesson",
                    &[
                        external_group_id,
                        &date.to_string(),
                        &lesson.start,
                        &lesson.end,
                        &lesson.name,
                        &lesson.teacher,
                        &lesson.room,
                    ],
                );

                let lesson_type = detect_lesson_type(&lesson.name);

                let source_data = json!({
                    "source": SOURCE,
                    "external_group_id": external_group_id,
                    "academic_week": if is_odd_week { "нечёт" } else { "чёт" },
                    "week_rule": lesson.week,
                    "pair": lesson.pair
                });

                sqlx::query(
                    r#"
                    INSERT INTO lessons (
                        id,
                        external_id,
                        group_id,
                        teacher_id,
                        room_id,
                        subject,
                        lesson_type,
                        starts_at,
                        ends_at,
                        week_day,
                        source_data,
                        updated_at
                    )
                    VALUES (
                        $1, $2, $3, $4, $5, $6, $7,
                        $8, $9, $10, $11, now()
                    )
                    ON CONFLICT (external_id)
                    DO UPDATE SET
                        group_id = EXCLUDED.group_id,
                        teacher_id = EXCLUDED.teacher_id,
                        room_id = EXCLUDED.room_id,
                        subject = EXCLUDED.subject,
                        lesson_type = EXCLUDED.lesson_type,
                        starts_at = EXCLUDED.starts_at,
                        ends_at = EXCLUDED.ends_at,
                        week_day = EXCLUDED.week_day,
                        source_data = EXCLUDED.source_data,
                        updated_at = now()
                    "#,
                )
                .bind(Uuid::new_v4())
                .bind(external_lesson_id)
                .bind(group_id)
                .bind(teacher_id)
                .bind(room_id)
                .bind(&lesson.name)
                .bind(lesson_type)
                .bind(starts_at)
                .bind(ends_at)
                .bind(date.weekday().number_from_monday() as i16)
                .bind(source_data)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

                imported += 1;
            }
        }

        date = date
            .succ_opt()
            .ok_or_else(|| "Could not advance schedule date".to_owned())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;

    Ok(imported)
}

async fn upsert_teacher(tx: &mut Transaction<'_, Postgres>, name: &str) -> Result<Uuid, String> {
    let external_id = stable_id("teacher", &[name.trim()]);

    sqlx::query_scalar(
        r#"
        INSERT INTO teachers (
            id, external_id, full_name, source_data, updated_at
        )
        VALUES ($1, $2, $3, '{"source":"eios"}'::jsonb, now())
        ON CONFLICT (external_id)
        DO UPDATE SET
            full_name = EXCLUDED.full_name,
            updated_at = now()
        RETURNING id
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(external_id)
    .bind(name.trim())
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| e.to_string())
}

async fn upsert_room(tx: &mut Transaction<'_, Postgres>, name: &str) -> Result<Uuid, String> {
    let external_id = stable_id("room", &[name.trim()]);

    sqlx::query_scalar(
        r#"
        INSERT INTO rooms (
            id, external_id, name, source_data, updated_at
        )
        VALUES ($1, $2, $3, '{"source":"eios"}'::jsonb, now())
        ON CONFLICT (external_id)
        DO UPDATE SET
            name = EXCLUDED.name,
            updated_at = now()
        RETURNING id
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(external_id)
    .bind(name.trim())
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| e.to_string())
}

fn local_midnight_utc(date: NaiveDate, offset: FixedOffset) -> Result<DateTime<Utc>, String> {
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "Invalid midnight time".to_owned())?;

    offset
        .from_local_datetime(&midnight)
        .single()
        .map(|value| value.with_timezone(&Utc))
        .ok_or_else(|| "Could not convert local midnight to UTC".to_owned())
}

fn local_datetime_utc(
    date: NaiveDate,
    time: NaiveTime,
    offset: FixedOffset,
) -> Result<DateTime<Utc>, String> {
    let local = date.and_time(time);

    offset
        .from_local_datetime(&local)
        .single()
        .map(|value| value.with_timezone(&Utc))
        .ok_or_else(|| "Could not convert lesson time to UTC".to_owned())
}

fn week_matches(rule: &str, is_odd_week: bool) -> bool {
    let normalized = rule.to_lowercase().replace('ё', "е");

    if normalized.contains("нечет") {
        is_odd_week
    } else if normalized.contains("чет") {
        !is_odd_week
    } else {
        true
    }
}

fn is_valid_lesson(lesson: &Lesson) -> bool {
    let subject = lesson.name.trim().to_lowercase();

    !subject.is_empty()
        && !subject.contains("загрузка")
        && !subject.contains("ошибка")
        && !subject.contains("нет данных")
        && NaiveTime::parse_from_str(&lesson.start, "%H:%M").is_ok()
        && NaiveTime::parse_from_str(&lesson.end, "%H:%M").is_ok()
}

fn detect_lesson_type(subject: &str) -> Option<String> {
    let lower = subject.to_lowercase();

    for suffix in ["лек.", "пр.", "лаб."] {
        if lower.ends_with(suffix) {
            return Some(suffix.to_owned());
        }
    }

    None
}

fn stable_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();

    hasher.update(prefix.as_bytes());

    for part in parts {
        hasher.update([0]);
        hasher.update(part.as_bytes());
    }

    let hash = hasher.finalize();

    let hex = hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    format!("eios:{prefix}:{hex}")
}
