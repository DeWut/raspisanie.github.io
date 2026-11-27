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
const DATABASE_SYNC_LOCK_KEY: i64 = 0x5350424655495453;
const WEEKS_AHEAD: i64 = 16;
static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
// 23 ноября 2026 года — понедельник нечётной учебной недели.
// При переходе на новый учебный семестр эту дату нужно будет проверить.
const ODD_WEEK_ANCHOR: (i32, u32, u32) = (2026, 11, 23);

const SOURCE: &str = "eios";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncReport {
    pub groups_total: usize,
    pub groups_successful: usize,
    pub groups_failed: usize,
    pub lessons_imported: usize,
}

fn remaining_sync_delay(last_synced_at: DateTime<Utc>, now: DateTime<Utc>) -> Duration {
    let elapsed = now
        .signed_duration_since(last_synced_at)
        .to_std()
        .unwrap_or(Duration::ZERO);
    SYNC_INTERVAL.saturating_sub(elapsed)
}

/// Runs immediately only when no previous sync exists, then follows the persisted 24-hour interval.
pub async fn run_daily_sync(pool: sqlx::PgPool, eios: EiosSync) {
    tracing::info!("Persistent EIOS schedule sync worker started");

    loop {
        let delay = match next_sync_delay(&pool).await {
            Ok(delay) => delay,
            Err(error) => {
                tracing::error!("Could not check EIOS sync history: {}", error);
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        match delay {
            Some(delay) if !delay.is_zero() => {
                tracing::info!(
                    "EIOS schedule was recently synchronized; next automatic run in {} seconds",
                    delay.as_secs()
                );
                tokio::time::sleep(delay).await;
                continue;
            }
            None => tracing::info!(
                "No previous EIOS schedule sync found; running the initial synchronization"
            ),
            _ => tracing::info!("The 24-hour EIOS sync interval has elapsed"),
        }

        tracing::info!("Starting automatic EIOS schedule synchronization");

        match sync_all_groups(&pool, &eios).await {
            Ok(report) => {
                tracing::info!(
                    "Automatic EIOS sync finished: {}/{} groups successful, {} failed, {} lessons imported",
                    report.groups_successful,
                    report.groups_total,
                    report.groups_failed,
                    report.lessons_imported
                );
            }
            Err(error) => {
                tracing::error!("Automatic EIOS schedule synchronization failed: {}", error);
            }
        }
    }
}

async fn next_sync_delay(pool: &sqlx::PgPool) -> Result<Option<Duration>, String> {
    let last_synced_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        "SELECT last_synced_at FROM sync_metadata WHERE source = 'eios_schedule'",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?
    .flatten();

    Ok(last_synced_at.map(|last_synced_at| remaining_sync_delay(last_synced_at, Utc::now())))
}

pub async fn sync_all_groups(pool: &sqlx::PgPool, eios: &EiosSync) -> Result<SyncReport, String> {
    sync_all_groups_with_progress(pool, eios, |_, _| {}).await
}

pub async fn sync_all_groups_with_progress<F>(
    pool: &sqlx::PgPool,
    eios: &EiosSync,
    mut on_progress: F,
) -> Result<SyncReport, String>
where
    F: FnMut(usize, usize) + Send,
{
    let _guard = SYNC_LOCK
        .try_lock()
        .map_err(|_| "Синхронизация уже выполняется".to_owned())?;
    // Use a transaction-scoped advisory lock. Unlike session-level locks,
    // pg_try_advisory_xact_lock works through Neon's PgBouncer transaction pool.
    // The transaction is kept alive with a lightweight SELECT between EIOS requests.
    let mut lock_tx = pool.begin().await.map_err(|error| error.to_string())?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
        .bind(DATABASE_SYNC_LOCK_KEY)
        .fetch_one(&mut *lock_tx)
        .await
        .map_err(|error| error.to_string())?;
    if !locked {
        lock_tx.rollback().await.map_err(|error| error.to_string())?;
        return Err("Синхронизация уже выполняется в другом процессе".to_owned());
    }

    let result = sync_all_groups_inner(pool, eios, &mut on_progress, &mut lock_tx).await;
    match lock_tx.commit().await {
        Ok(()) => result,
        Err(error) => Err(format!("Не удалось корректно завершить транзакцию блокировки синхронизации: {error}")),
    }
}

async fn sync_all_groups_inner<F>(
    pool: &sqlx::PgPool,
    eios: &EiosSync,
    on_progress: &mut F,
    lock_tx: &mut sqlx::Transaction<'_, Postgres>,
) -> Result<SyncReport, String>
where
    F: FnMut(usize, usize),
{
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
    let total = rows.len();
    on_progress(0, total);

    tracing::info!("Active groups found for schedule sync: {}", rows.len());

    for row in rows {
        // Keep the lock transaction active while a potentially slow EIOS request runs.
        sqlx::query("SELECT 1").execute(&mut **lock_tx).await.map_err(|error| error.to_string())?;
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
                on_progress(successful + failed, total);
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
            on_progress(successful + failed, total);
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
        on_progress(successful + failed, total);

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

    on_progress(total, total);

    Ok(SyncReport {
        groups_total: successful + failed,
        groups_successful: successful,
        groups_failed: failed,
        lessons_imported: imported_lessons,
    })
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

    // Snapshot the current schedule before upserting, so room/teacher edits
    // can be compared and students can opt in to change alerts.
    let old_rows = sqlx::query_as::<_, (DateTime<Utc>, DateTime<Utc>, String, Option<String>, Option<String>)>(
        r#"SELECT l.starts_at,l.ends_at,l.subject,r.name,t.full_name
           FROM lessons l LEFT JOIN rooms r ON r.id=l.room_id LEFT JOIN teachers t ON t.id=l.teacher_id
           WHERE l.group_id=$1 AND l.starts_at >= $2 AND l.starts_at < $3 AND l.source_data->>'source'='eios'"#
    ).bind(group_id).bind(start_bound).bind(end_bound).fetch_all(&mut *tx).await.map_err(|e| e.to_string())?;
    let natural_key = |starts: DateTime<Utc>, ends: DateTime<Utc>, subject: &str| {
        format!(
            "{}|{}|{}",
            starts.to_rfc3339(),
            ends.to_rfc3339(),
            subject.trim().to_lowercase()
        )
    };
    let old_by_key: HashMap<String, (Option<String>, Option<String>)> = old_rows
        .iter()
        .map(|(starts, ends, subject, room, teacher)| {
            (
                natural_key(*starts, *ends, subject),
                (room.clone(), teacher.clone()),
            )
        })
        .collect();
    let mut new_natural_keys = std::collections::HashSet::<String>::new();

    // Сначала обновляем существующие занятия по стабильному external_id.
    // Это сохраняет UUID и историю уже отправленных напоминаний.
    let mut seen_external_ids = Vec::<String>::new();
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

                let key = natural_key(starts_at, ends_at, &lesson.name);
                new_natural_keys.insert(key.clone());
                if let Some((old_room, old_teacher)) = old_by_key.get(&key) {
                    for (kind, old_value, new_value) in [
                        (
                            "room_changed",
                            old_room.as_deref(),
                            (!lesson.room.trim().is_empty()).then_some(lesson.room.trim()),
                        ),
                        (
                            "teacher_changed",
                            old_teacher.as_deref(),
                            (!lesson.teacher.trim().is_empty()).then_some(lesson.teacher.trim()),
                        ),
                    ] {
                        if old_value != new_value {
                            let change_key = stable_id(
                                "schedule-change",
                                &[
                                    &group_id.to_string(),
                                    kind,
                                    &key,
                                    old_value.unwrap_or(""),
                                    new_value.unwrap_or(""),
                                ],
                            );
                            sqlx::query("INSERT INTO bot_schedule_changes(group_id,change_key,change_kind,starts_at,subject,old_room,new_room,old_teacher,new_teacher) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(change_key) DO NOTHING")
                                .bind(group_id).bind(change_key).bind(kind).bind(starts_at).bind(&lesson.name)
                                .bind(if kind=="room_changed" { old_value } else { None })
                                .bind(if kind=="room_changed" { new_value } else { None })
                                .bind(if kind=="teacher_changed" { old_value } else { None })
                                .bind(if kind=="teacher_changed" { new_value } else { None })
                                .execute(&mut *tx).await.map_err(|e| e.to_string())?;
                        }
                    }
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

                seen_external_ids.push(external_lesson_id.clone());
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

    // Удаляем устаревшие занятия только после успешного импорта новых данных.
    // При пустом результате старое расписание остаётся нетронутым.
    if !seen_external_ids.is_empty() {
        for (starts, ends, subject, room, teacher) in &old_rows {
            if !new_natural_keys.contains(&natural_key(*starts, *ends, subject)) {
                let key = natural_key(*starts, *ends, subject);
                let change_key = stable_id(
                    "schedule-change",
                    &[&group_id.to_string(), "class_cancelled", &key],
                );
                sqlx::query("INSERT INTO bot_schedule_changes(group_id,change_key,change_kind,starts_at,subject,old_room,old_teacher) VALUES($1,$2,'class_cancelled',$3,$4,$5,$6) ON CONFLICT(change_key) DO NOTHING")
                    .bind(group_id).bind(change_key).bind(*starts).bind(subject).bind(room).bind(teacher)
                    .execute(&mut *tx).await.map_err(|e| e.to_string())?;
            }
        }
        sqlx::query(
            r#"
            DELETE FROM lessons
            WHERE group_id = $1
              AND starts_at >= $2
              AND starts_at < $3
              AND source_data->>'source' = 'eios'
              AND NOT (external_id = ANY($4))
            "#,
        )
        .bind(group_id)
        .bind(start_bound)
        .bind(end_bound)
        .bind(&seen_external_ids)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
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
            id, external_id, name, building, source_data, updated_at
        )
        VALUES ($1, $2, $3, substring($3 from '^([0-9]+)\.'), '{"source":"eios"}'::jsonb, now())
        ON CONFLICT (external_id)
        DO UPDATE SET
            name = EXCLUDED.name,
            building = COALESCE(EXCLUDED.building, rooms.building),
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

#[cfg(test)]
mod sync_worker_tests {
    use super::*;

    #[test]
    fn automatic_sync_waits_for_the_persisted_24_hour_interval() {
        let now = Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap();
        let last_sync = now - ChronoDuration::hours(23);
        assert_eq!(
            remaining_sync_delay(last_sync, now),
            Duration::from_secs(60 * 60)
        );
    }

    #[test]
    fn automatic_sync_runs_when_the_interval_has_elapsed() {
        let now = Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap();
        let last_sync = now - ChronoDuration::hours(25);
        assert_eq!(remaining_sync_delay(last_sync, now), Duration::ZERO);
    }
}
