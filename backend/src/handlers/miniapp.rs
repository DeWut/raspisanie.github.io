use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{NaiveTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use teloxide::prelude::*;

use crate::{auth::AuthUser, error::AppError, state::SharedState};

async fn telegram_user_id(
    state: &SharedState,
    auth: &AuthUser,
) -> Result<(uuid::Uuid, i64), AppError> {
    let user_id = super::user_id(state, auth).await?;
    Ok((user_id, auth.telegram_id))
}

fn allowed_reminder_minutes(value: i32) -> bool {
    matches!(value, 10 | 60 | 180 | 1440)
}

fn parse_clock(value: &str) -> Result<NaiveTime, AppError> {
    NaiveTime::parse_from_str(value, "%H:%M").map_err(|_| AppError::BadRequest)
}

pub async fn get_preferences(
    State(state): State<SharedState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (user_id, _) = telegram_user_id(&state, &auth).await?;
    let row = sqlx::query_as::<_, (Option<uuid::Uuid>, Option<String>, Value)>(
        r#"
        SELECT u.group_id, g.name, u.settings
        FROM users u
        LEFT JOIN groups g ON g.id=u.group_id
        WHERE u.id=$1
        "#,
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    let settings = &row.2;
    Ok(Json(json!({
        "group_id": row.0,
        "group_name": row.1,
        "settings": {
            "reminders_enabled": settings["reminders_enabled"] == true,
            "reminder_minutes": settings["reminder_minutes"].as_i64().unwrap_or(10).clamp(10, 1440),
            "daily_digest_enabled": settings["daily_digest_enabled"] == true,
            "daily_digest_time": settings["daily_digest_time"].as_str().unwrap_or("07:00"),
            "schedule_changes_enabled": settings["schedule_changes_enabled"] == true
        },
        "timezone": "Europe/Moscow"
    })))
}

#[derive(Deserialize)]
pub struct PreferencesPatch {
    pub reminders_enabled: Option<bool>,
    pub reminder_minutes: Option<i32>,
    pub daily_digest_enabled: Option<bool>,
    pub daily_digest_time: Option<String>,
    pub schedule_changes_enabled: Option<bool>,
}

pub async fn update_preferences(
    State(state): State<SharedState>,
    auth: AuthUser,
    Json(body): Json<PreferencesPatch>,
) -> Result<Json<Value>, AppError> {
    let (user_id, _) = telegram_user_id(&state, &auth).await?;
    if body.reminder_minutes.is_some_and(|value| !allowed_reminder_minutes(value)) {
        return Err(AppError::BadRequest);
    }
    if let Some(value) = body.daily_digest_time.as_deref() {
        parse_clock(value)?;
    }

    let mut patch = serde_json::Map::new();
    if let Some(value) = body.reminders_enabled {
        patch.insert("reminders_enabled".into(), json!(value));
    }
    if let Some(value) = body.reminder_minutes {
        patch.insert("reminder_minutes".into(), json!(value));
    }
    if let Some(value) = body.daily_digest_enabled {
        patch.insert("daily_digest_enabled".into(), json!(value));
    }
    if let Some(value) = body.daily_digest_time {
        patch.insert("daily_digest_time".into(), json!(value));
    }
    if let Some(value) = body.schedule_changes_enabled {
        patch.insert("schedule_changes_enabled".into(), json!(value));
    }
    if patch.is_empty() {
        return Err(AppError::BadRequest);
    }

    let settings: Value = sqlx::query_scalar(
        "UPDATE users SET settings=COALESCE(settings,'{}'::jsonb) || $2::jsonb,updated_at=now() WHERE id=$1 RETURNING settings",
    )
    .bind(user_id)
    .bind(Value::Object(patch))
    .fetch_one(&state.db)
    .await?;

    Ok(Json(json!({"settings": settings})))
}

async fn can_manage_chat(
    state: &SharedState,
    chat_id: i64,
    telegram_id: i64,
) -> Result<bool, AppError> {
    let token = state.config.teloxide_token.as_deref().ok_or(AppError::Internal)?;
    let bot = Bot::new(token);
    let role = crate::services::chat_roles::sync_member(
        &bot,
        &state.db,
        ChatId(chat_id),
        telegram_id,
        None,
    )
    .await
    .map_err(|error| {
        tracing::warn!(
            chat_id,
            telegram_id,
            "Could not verify Telegram admin status for Mini App settings: {}",
            error
        );
        AppError::Forbidden
    })?;

    Ok(matches!(role.as_deref(), Some("owner" | "admin")))
}

pub async fn list_managed_chats(
    State(state): State<SharedState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (_, telegram_id) = telegram_user_id(&state, &auth).await?;
    let rows = sqlx::query_as::<_, (i64, Option<String>, String, bool, bool, bool, bool)>(
        r#"
        SELECT c.chat_id, c.chat_title, g.name,
               c.daily_schedule_enabled, c.lesson_notifications_enabled,
               c.daily_digest_enabled,
               (c.owner_telegram_id=$1 OR m.telegram_status IN ('creator','administrator')) AS can_manage
        FROM bot_chat_settings c
        JOIN groups g ON g.id=c.group_id
        LEFT JOIN bot_chat_members m
          ON m.chat_id=c.chat_id AND m.telegram_id=$1
        LEFT JOIN users u ON u.telegram_id=$1
        WHERE c.is_active=TRUE
          AND (
            c.owner_telegram_id=$1
            OR m.telegram_id IS NOT NULL
            OR u.group_id=c.group_id
          )
        ORDER BY COALESCE(c.chat_title,g.name),c.chat_id
        "#,
    )
    .bind(telegram_id)
    .fetch_all(&state.db)
    .await?;

    let chats = rows
        .into_iter()
        .map(|(chat_id, title, group_name, daily, lesson, digest, can_manage)| {
            json!({
                "chat_id": chat_id,
                "title": title.unwrap_or_else(|| group_name.clone()),
                "group_name": group_name,
                "daily_schedule_enabled": daily,
                "lesson_notifications_enabled": lesson,
                "daily_digest_enabled": digest,
                "can_manage": can_manage
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({"chats": chats})))
}

pub async fn get_chat_settings(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path(chat_id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let (_, telegram_id) = telegram_user_id(&state, &auth).await?;
    if !can_manage_chat(&state, chat_id, telegram_id).await? {
        return Err(AppError::Forbidden);
    }

    let row = sqlx::query(
        r#"
        SELECT g.name, g.external_id, c.daily_schedule_enabled,
               to_char(c.daily_schedule_time,'HH24:MI') AS daily_schedule_time,
               c.lesson_notifications_enabled, c.daily_digest_enabled,
               to_char(c.daily_digest_time,'HH24:MI') AS daily_digest_time,
               c.assistant_enabled, c.context_enabled, c.context_retention_days,
               c.homework_autocollect_enabled, c.homework_approver_role,
               c.homework_reminders_enabled, c.exam_reminders_enabled,
               c.event_reminders_enabled,
               c.schedule_changes_enabled, c.memes_enabled, c.timezone,
               c.content_permissions, c.settings
        FROM bot_chat_settings c
        JOIN groups g ON g.id=c.group_id
        WHERE c.chat_id=$1 AND c.is_active=TRUE
        "#,
    )
    .bind(chat_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    use sqlx::Row;
    let settings: Value = row.try_get("settings")?;
    let permissions: Value = row.try_get("content_permissions")?;
    Ok(Json(json!({
        "chat_id": chat_id,
        "group_name": row.try_get::<String,_>("name")?,
        "external_id": row.try_get::<String,_>("external_id")?,
        "daily_schedule_enabled": row.try_get::<bool,_>("daily_schedule_enabled")?,
        "daily_schedule_time": row.try_get::<String,_>("daily_schedule_time")?,
        "lesson_notifications_enabled": row.try_get::<bool,_>("lesson_notifications_enabled")?,
        "daily_digest_enabled": row.try_get::<bool,_>("daily_digest_enabled")?,
        "daily_digest_time": row.try_get::<String,_>("daily_digest_time")?,
        "assistant_enabled": row.try_get::<bool,_>("assistant_enabled")?,
        "context_enabled": row.try_get::<bool,_>("context_enabled")?,
        "context_retention_days": row.try_get::<i16,_>("context_retention_days")?,
        "homework_autocollect_enabled": row.try_get::<bool,_>("homework_autocollect_enabled")?,
        "homework_approver_role": row.try_get::<String,_>("homework_approver_role")?,
        "homework_reminders_enabled": row.try_get::<bool,_>("homework_reminders_enabled")?,
        "exam_reminders_enabled": row.try_get::<bool,_>("exam_reminders_enabled")?,
        "event_reminders_enabled": row.try_get::<bool,_>("event_reminders_enabled")?,
        "schedule_changes_enabled": row.try_get::<bool,_>("schedule_changes_enabled")?,
        "memes_enabled": row.try_get::<bool,_>("memes_enabled")?,
        "timezone": row.try_get::<String,_>("timezone")?,
        "content_permissions": permissions,
        "spam_filter_enabled": settings["spam_filter_enabled"] == true
    })))
}

#[derive(Deserialize)]
pub struct ChatSettingsPatch {
    pub daily_schedule_enabled: Option<bool>,
    pub daily_schedule_time: Option<String>,
    pub lesson_notifications_enabled: Option<bool>,
    pub daily_digest_enabled: Option<bool>,
    pub daily_digest_time: Option<String>,
    pub assistant_enabled: Option<bool>,
    pub context_enabled: Option<bool>,
    pub context_retention_days: Option<i16>,
    pub consent_context: Option<bool>,
    pub homework_autocollect_enabled: Option<bool>,
    pub homework_approver_role: Option<String>,
    pub homework_reminders_enabled: Option<bool>,
    pub exam_reminders_enabled: Option<bool>,
    pub event_reminders_enabled: Option<bool>,
    pub schedule_changes_enabled: Option<bool>,
    pub memes_enabled: Option<bool>,
    pub spam_filter_enabled: Option<bool>,
    pub content_permissions: Option<Value>,
}

pub async fn update_chat_settings(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path(chat_id): Path<i64>,
    Json(body): Json<ChatSettingsPatch>,
) -> Result<Json<Value>, AppError> {
    let (_, telegram_id) = telegram_user_id(&state, &auth).await?;
    if !can_manage_chat(&state, chat_id, telegram_id).await? {
        return Err(AppError::Forbidden);
    }
    let context_already_enabled: bool = sqlx::query_scalar(
        "SELECT context_enabled FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE",
    )
    .bind(chat_id)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(false);
    if body.context_enabled == Some(true)
        && body.consent_context != Some(true)
        && !context_already_enabled
    {
        return Err(AppError::BadRequest);
    }
    if body.context_retention_days.is_some_and(|days| !(1..=30).contains(&days)) {
        return Err(AppError::BadRequest);
    }
    if body.homework_approver_role.as_deref().is_some_and(|role| {
        !matches!(role, "class_leader" | "moderator" | "admin")
    }) {
        return Err(AppError::BadRequest);
    }
    if let Some(permissions) = body.content_permissions.as_ref() {
        let Some(object) = permissions.as_object() else {
            return Err(AppError::BadRequest);
        };
        if object.iter().any(|(section, value)| {
            !["homework", "exams", "events"].contains(&section.as_str())
                || !value.as_str().is_some_and(|mode| matches!(mode, "everyone" | "admins" | "selected"))
        }) {
            return Err(AppError::BadRequest);
        }
    }

    let daily_schedule_time = body.daily_schedule_time.as_deref().map(parse_clock).transpose()?;
    let daily_digest_time = body.daily_digest_time.as_deref().map(parse_clock).transpose()?;

    let mut tx = state.db.begin().await?;
    sqlx::query(
        r#"
        UPDATE bot_chat_settings SET
          daily_schedule_enabled=COALESCE($2,daily_schedule_enabled),
          daily_schedule_time=COALESCE($3,daily_schedule_time),
          lesson_notifications_enabled=COALESCE($4,lesson_notifications_enabled),
          daily_digest_enabled=COALESCE($5,daily_digest_enabled),
          daily_digest_time=COALESCE($6,daily_digest_time),
          assistant_enabled=COALESCE($7,assistant_enabled),
          context_enabled=COALESCE($8,context_enabled),
          context_retention_days=COALESCE($9,context_retention_days),
          homework_autocollect_enabled=COALESCE($10,homework_autocollect_enabled),
          homework_approver_role=COALESCE($11,homework_approver_role),
          schedule_changes_enabled=COALESCE($12,schedule_changes_enabled),
          memes_enabled=COALESCE($13,memes_enabled),
          settings=CASE WHEN $14::boolean IS NULL THEN settings
                        ELSE jsonb_set(COALESCE(settings,'{}'::jsonb),'{spam_filter_enabled}',to_jsonb($14::boolean),true) END,
          content_permissions=COALESCE(content_permissions,'{}'::jsonb) || COALESCE($15::jsonb,'{}'::jsonb),
          homework_reminders_enabled=COALESCE($16,homework_reminders_enabled),
          exam_reminders_enabled=COALESCE($17,exam_reminders_enabled),
          event_reminders_enabled=COALESCE($18,event_reminders_enabled),
          timezone='Europe/Moscow',
          updated_at=now()
        WHERE chat_id=$1 AND is_active=TRUE
        "#,
    )
    .bind(chat_id)
    .bind(body.daily_schedule_enabled)
    .bind(daily_schedule_time)
    .bind(body.lesson_notifications_enabled)
    .bind(body.daily_digest_enabled)
    .bind(daily_digest_time)
    .bind(body.assistant_enabled)
    .bind(body.context_enabled)
    .bind(body.context_retention_days)
    .bind(body.homework_autocollect_enabled)
    .bind(body.homework_approver_role.as_deref())
    .bind(body.schedule_changes_enabled)
    .bind(body.memes_enabled)
    .bind(body.spam_filter_enabled)
    .bind(body.content_permissions)
    .bind(body.homework_reminders_enabled)
    .bind(body.exam_reminders_enabled)
    .bind(body.event_reminders_enabled)
    .execute(&mut *tx)
    .await?;

    if body.context_enabled == Some(false) {
        sqlx::query("DELETE FROM bot_chat_context_messages WHERE chat_id=$1")
            .bind(chat_id)
            .execute(&mut *tx)
            .await?;
    }
    if body.context_enabled.is_some()
        || body.assistant_enabled.is_some()
        || body.context_retention_days.is_some()
    {
        let detail = json!({
            "assistant_enabled": body.assistant_enabled,
            "context_enabled": body.context_enabled,
            "context_retention_days": body.context_retention_days,
            "consent": body.consent_context == Some(true) || (body.context_enabled == Some(true) && context_already_enabled)
        });
        sqlx::query(
            "INSERT INTO bot_chat_assistant_settings_audit(chat_id,changed_by,setting,value) VALUES($1,$2,'miniapp_settings',$3)",
        )
        .bind(chat_id)
        .bind(telegram_id)
        .bind(detail)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO bot_chat_audit_logs(chat_id,actor_telegram_id,action,details) VALUES($1,$2,'miniapp_settings_updated','{}'::jsonb)",
    )
    .bind(chat_id)
    .bind(telegram_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get_chat_settings(State(state), auth, Path(chat_id)).await
}

#[derive(Deserialize)]
pub struct ChatGroupUpdate {
    pub external_id: String,
}

pub async fn update_chat_group(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path(chat_id): Path<i64>,
    Json(body): Json<ChatGroupUpdate>,
) -> Result<Json<Value>, AppError> {
    let (_, telegram_id) = telegram_user_id(&state, &auth).await?;
    if !can_manage_chat(&state, chat_id, telegram_id).await? {
        return Err(AppError::Forbidden);
    }
    if body.external_id.is_empty()
        || body.external_id.len() > 32
        || !body.external_id.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(AppError::BadRequest);
    }
    let group = sqlx::query_as::<_, (uuid::Uuid, String)>(
        "SELECT id,name FROM groups WHERE external_id=$1 AND is_active=TRUE",
    )
    .bind(&body.external_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    sqlx::query(
        "UPDATE bot_chat_settings SET group_id=$2,configured_by=$3,updated_at=now() WHERE chat_id=$1 AND is_active=TRUE",
    )
    .bind(chat_id)
    .bind(group.0)
    .bind(telegram_id)
    .execute(&state.db)
    .await?;
    Ok(Json(json!({
        "chat_id": chat_id,
        "group_id": group.0,
        "group_name": group.1,
        "external_id": body.external_id
    })))
}

#[derive(Deserialize)]
pub struct ChatCommand {
    pub command: String,
    #[serde(default)]
    pub argument: String,
}

pub async fn run_chat_command(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path(chat_id): Path<i64>,
    Json(body): Json<ChatCommand>,
) -> Result<Json<Value>, AppError> {
    let (_, telegram_id) = telegram_user_id(&state, &auth).await?;
    if chat_id >= 0 {
        return Err(AppError::BadRequest);
    }
    let bot_token = state.config.teloxide_token.as_deref().ok_or(AppError::Internal)?;
    let bot = Bot::new(bot_token);
    let role = crate::services::chat_roles::sync_member(
        &bot,
        &state.db,
        ChatId(chat_id),
        telegram_id,
        None,
    )
    .await
    .map_err(|_| AppError::Forbidden)?;
    if role.is_none() {
        return Err(AppError::Forbidden);
    }

    let command = body.command.trim();
    let argument = body.argument.trim();
    let allowed = matches!(
        command,
        "/dz" | "/homework" | "/add_dz" | "/edit_dz" | "/delete_dz"
            | "/done" | "/my_dz" | "/priority_dz"
            | "/exams" | "/add_exam" | "/edit_exam" | "/exam_info" | "/delete_exam"
            | "/events" | "/add_event" | "/edit_event" | "/cancel_event"
            | "/join_event" | "/leave_event" | "/event_capacity" | "/event_waitlist"
            | "/event_attendees" | "/event_maybe" | "/event_no"
            | "/event_reminders_on" | "/event_reminders_off"
            | "/content_rights" | "/content_editor"
            | "/search" | "/group_stats" | "/my_stats"
            | "/resources" | "/add_resource" | "/lost_found" | "/lost" | "/found"
            | "/autodz_on" | "/autodz_off" | "/spam_filter_on" | "/spam_filter_off"
            | "/autodz_approver" | "/subject_alias" | "/subject_aliases"
            | "/report_queue" | "/report_resolve"
    );
    if !allowed || !command.starts_with('/') || command.chars().any(char::is_whitespace) {
        return Err(AppError::BadRequest);
    }
    if argument.chars().count() > 4000 {
        return Err(AppError::BadRequest);
    }

    let response = if let Some(text) = crate::services::group_content::handle_homework(
        &state.db, chat_id, telegram_id, command, argument,
    ).await {
        Some(text)
    } else if let Some(text) = crate::services::group_content::handle_exams_events(
        &bot, &state.db, chat_id, telegram_id, command, argument,
    ).await {
        Some(text)
    } else if let Some(text) = crate::services::group_content::handle_content_admin(
        &bot, &state.db, chat_id, telegram_id, command, argument,
    ).await {
        Some(text)
    } else if let Some(text) = crate::services::group_content::handle_group_tools(
        &state.db, chat_id, telegram_id, command, argument,
    ).await {
        Some(text)
    } else {
        crate::services::student_tools::handle(
            &state.db, chat_id, telegram_id, command, argument, None, None,
        ).await
    };

    let response = response.unwrap_or_else(|| {
        "Эта операция требует сообщения или вложения непосредственно в Telegram-чате. Остальные функции доступны здесь.".into()
    });
    Ok(Json(json!({"command": command, "response": response})))
}

#[derive(Deserialize)]
pub struct AssistantRequest {
    pub message: String,
}

pub async fn assistant_chat(
    State(state): State<SharedState>,
    auth: AuthUser,
    Json(body): Json<AssistantRequest>,
) -> Result<Json<Value>, AppError> {
    let (_, telegram_id) = telegram_user_id(&state, &auth).await?;
    let prompt = body.message.trim();
    if prompt.is_empty() || prompt.chars().count() > 600 {
        return Err(AppError::BadRequest);
    }

    let group_id = sqlx::query_scalar::<_, Option<uuid::Uuid>>(
        "SELECT group_id FROM users WHERE telegram_id=$1",
    )
    .bind(telegram_id)
    .fetch_one(&state.db)
    .await?;

    let mut intent = crate::services::conversation::detect(prompt);
    if intent.is_none() {
        let lower = prompt.to_lowercase().replace("ё", "е");
        let is_subject_followup = lower.starts_with("а по ") || lower.starts_with("и по ");
        if is_subject_followup {
            let previous = sqlx::query_scalar::<_, String>(
                "SELECT content FROM bot_user_assistant_messages WHERE telegram_id=$1 AND role='assistant' ORDER BY id DESC LIMIT 1",
            )
            .bind(telegram_id)
            .fetch_optional(&state.db)
            .await?;
            if previous.as_deref().is_some_and(|value| {
                let topic = value.to_lowercase();
                topic.contains("домаш") || topic.contains("задан") || topic.contains("дз")
            }) {
                intent = crate::services::conversation::homework_subject(prompt)
                    .map(crate::services::conversation::Intent::HomeworkFor);
            }
        }
    }
    let (kind, reply) = match intent {
        Some(crate::services::conversation::Intent::Greeting) => (
            "greeting",
            "Привет! Я учебный помощник СПбГЛТУ. Могу подсказать расписание, следующую пару, домашние задания, дедлайны, преподавателей и свободные аудитории.".to_owned(),
        ),
        Some(crate::services::conversation::Intent::Help) => (
            "help",
            "Спроси, например: «какое расписание завтра?», «что задали?», «какие дедлайны?», «когда пары у Иванова?» или «свободные аудитории на 15:00». Для данных своей группы привяжи её в настройках.".to_owned(),
        ),
        Some(crate::services::conversation::Intent::Today) => {
            let Some(group) = group_id else {
                return Ok(Json(json!({"kind":"schedule","reply":"Сначала выбери учебную группу в настройках Mini App."})));
            };
            ("schedule", crate::bot::schedule_text(&state.db, group, "/today").await)
        }
        Some(crate::services::conversation::Intent::Tomorrow) => {
            let Some(group) = group_id else {
                return Ok(Json(json!({"kind":"schedule","reply":"Сначала выбери учебную группу в настройках Mini App."})));
            };
            ("schedule", crate::bot::schedule_text(&state.db, group, "/tomorrow").await)
        }
        Some(crate::services::conversation::Intent::Week) => {
            let Some(group) = group_id else {
                return Ok(Json(json!({"kind":"schedule","reply":"Сначала выбери учебную группу в настройках Mini App."})));
            };
            ("schedule", crate::bot::schedule_text(&state.db, group, "/week").await)
        }
        Some(crate::services::conversation::Intent::NextLesson) => {
            let Some(group) = group_id else {
                return Ok(Json(json!({"kind":"schedule","reply":"Сначала выбери учебную группу в настройках Mini App."})));
            };
            ("schedule", crate::bot::schedule_text(&state.db, group, "/next").await)
        }
        Some(crate::services::conversation::Intent::Homework) => {
            let Some(group) = group_id else {
                return Ok(Json(json!({"kind":"homework","reply":"Сначала выбери учебную группу в настройках Mini App."})));
            };
            ("homework", crate::services::group_content::homework_for_group(&state.db, group, telegram_id).await)
        }
        Some(crate::services::conversation::Intent::HomeworkFor(subject)) => {
            let Some(group) = group_id else {
                return Ok(Json(json!({"kind":"homework","reply":"Сначала выбери учебную группу в настройках Mini App."})));
            };
            ("homework", crate::services::group_content::homework_for_group_subject(&state.db, group, telegram_id, &subject).await)
        }
        Some(crate::services::conversation::Intent::Deadlines) => (
            "deadlines",
            crate::services::student_tools::handle(&state.db, telegram_id, telegram_id, "/deadlines", "", None, None)
                .await.unwrap_or_else(|| "Пока нет доступных дедлайнов.".into()),
        ),
        Some(crate::services::conversation::Intent::FreeRooms(time)) => (
            "rooms",
            crate::services::student_tools::handle(&state.db, telegram_id, telegram_id, "/free_rooms", time.as_deref().unwrap_or(""), None, None)
                .await.unwrap_or_else(|| "Не удалось получить список свободных аудиторий.".into()),
        ),
        Some(crate::services::conversation::Intent::Teacher(query)) => (
            "teacher",
            crate::services::student_tools::handle(&state.db, telegram_id, telegram_id, "/teacher", &query, None, None)
                .await.unwrap_or_else(|| "Не удалось найти преподавателя.".into()),
        ),
        None => {
            let lower = prompt.to_lowercase().replace("ё", "е");
            let smalltalk = if lower.contains("как дела") || lower.contains("как ты") {
                Some("Работаю штатно и готов помочь с учёбой. А у тебя что по расписанию?")
            } else if lower == "спасибо" || lower.starts_with("спасибо ") || lower.contains("благодарю") {
                Some("Пожалуйста. Обращайся, когда понадобится расписание, ДЗ или помощь с дедлайнами.")
            } else if lower == "пока" || lower.contains("до свидания") || lower.contains("до встречи") {
                Some("Пока! Удачи с учёбой. Я буду здесь, когда понадобится помощь.")
            } else if lower.contains("кто ты") || lower.contains("что ты умеешь") {
                Some("Я учебный помощник SPBFTUIT. Работаю с расписанием, следующими парами, домашними заданиями, дедлайнами, преподавателями и свободными аудиториями.")
            } else if lower.contains("доброе утро") {
                Some("Доброе утро! Могу проверить расписание на сегодня и ближайшие пары.")
            } else if lower.contains("добрый вечер") {
                Some("Добрый вечер! Могу помочь проверить задания и расписание на завтра.")
            } else {
                None
            };
            if let Some(response) = smalltalk {
                ("conversation", response.to_owned())
            } else {
                let followup = lower.starts_with("а ")
                    || lower.starts_with("и ")
                    || lower.starts_with("почему ")
                    || lower.starts_with("а если")
                    || lower.starts_with("а когда");
                let previous = if followup {
                    sqlx::query_scalar::<_, String>(
                        "SELECT content FROM bot_user_assistant_messages WHERE telegram_id=$1 AND role='assistant' ORDER BY id DESC LIMIT 1",
                    )
                    .bind(telegram_id)
                    .fetch_optional(&state.db)
                    .await?
                } else {
                    None
                };
                let response = if let Some(previous) = previous {
                    let topic = previous.to_lowercase();
                    if topic.contains("расписан") || topic.contains("пара") {
                        "Понял продолжение вопроса о расписании. Уточни день: сегодня, завтра или вся неделя.".to_owned()
                    } else if topic.contains("домаш") || topic.contains("задан") || topic.contains("дз") {
                        "Продолжим про задания. Напиши предмет или уточни, какие сроки тебя интересуют.".to_owned()
                    } else if topic.contains("дедлайн") || topic.contains("срок") {
                        "Продолжим про сроки. Уточни предмет или напиши «ближайшие дедлайны».".to_owned()
                    } else {
                        "Я могу поддерживать короткий диалог по учебным вопросам. Спроси про расписание, ДЗ, дедлайны, преподавателей или аудитории.".to_owned()
                    }
                } else {
                    "Я учебный помощник, поэтому храню только короткий контекст, связанный с учёбой. Могу помочь с расписанием, ДЗ, дедлайнами, преподавателями и свободными аудиториями.".to_owned()
                };
                ("conversation", response)
            }
        }
    };

    // Store only education-related exchanges. Personal, unrelated messages are not retained.
    if !matches!(kind, "greeting" | "help" | "conversation") {
        let mut tx = state.db.begin().await?;
        sqlx::query(
            "INSERT INTO bot_user_assistant_messages(telegram_id,role,content) VALUES($1,'user',$2),($1,'assistant',$3)",
        )
        .bind(telegram_id)
        .bind(prompt.chars().take(600).collect::<String>())
        .bind(reply.chars().take(4000).collect::<String>())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM bot_user_assistant_messages WHERE telegram_id=$1 AND created_at<now()-interval '7 days'",
        )
        .bind(telegram_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM bot_user_assistant_messages WHERE telegram_id=$1 AND id IN (SELECT id FROM bot_user_assistant_messages WHERE telegram_id=$1 ORDER BY id DESC OFFSET 24)",
        )
        .bind(telegram_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
    }

    Ok(Json(json!({"kind": kind, "reply": reply, "engine": "local_rules"})))
}

pub async fn notification_health(
    State(state): State<SharedState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (_, telegram_id) = telegram_user_id(&state, &auth).await?;
    let worker = sqlx::query_as::<_, (Option<chrono::DateTime<Utc>>, Option<chrono::DateTime<Utc>>, Option<String>)>(
        "SELECT last_tick_at,last_success_at,last_error FROM bot_notification_worker_state WHERE worker='telegram_reminders'",
    )
    .fetch_optional(&state.db)
    .await?;

    let rows = sqlx::query_as::<_, (String, String, i64, chrono::DateTime<Utc>, Option<String>)>(
        r#"
        SELECT kind, status, recipient_id, created_at, detail
        FROM bot_notification_delivery_log
        WHERE recipient_kind='private' AND recipient_id=$1
           OR recipient_kind='group' AND recipient_id IN (
             SELECT c.chat_id FROM bot_chat_settings c
             LEFT JOIN bot_chat_members m ON m.chat_id=c.chat_id AND m.telegram_id=$1
             WHERE c.owner_telegram_id=$1 OR m.telegram_status IN ('creator','administrator')
           )
        ORDER BY created_at DESC
        LIMIT 30
        "#,
    )
    .bind(telegram_id)
    .fetch_all(&state.db)
    .await?;

    let deliveries = rows.into_iter().map(|(kind,status,recipient_id,created_at,detail)| {
        json!({"kind":kind,"status":status,"recipient_id":recipient_id,"created_at":created_at,"detail":detail})
    }).collect::<Vec<_>>();

    let worker = worker.map(|(tick,success,error)| {
        let healthy = tick.as_ref().is_some_and(|value| {
            Utc::now().signed_duration_since(*value).num_seconds() < 120
        });
        json!({
            "last_tick_at":tick,
            "last_success_at":success,
            "has_delivery_error":error.is_some(),
            "last_error":error.as_ref().map(|_| "Одна из последних отправок завершилась ошибкой; подробности доступны в журнале твоих доставок"),
            "healthy":healthy
        })
    }).unwrap_or_else(||json!({"healthy":false,"last_tick_at":null,"last_success_at":null,"last_error":"Ожидается первый запуск фонового цикла"}));

    Ok(Json(json!({"worker":worker,"deliveries":deliveries})))
}

pub async fn send_notification_test(
    State(state): State<SharedState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (_, telegram_id) = telegram_user_id(&state, &auth).await?;
    let token = state.config.teloxide_token.as_deref().ok_or(AppError::Internal)?;
    let bot = Bot::new(token);
    let result = bot.send_message(
        ChatId(telegram_id),
        "✅ Тестовое уведомление SPBFTUIT доставлено. Канал связи с Telegram работает.",
    ).await;

    match result {
        Ok(_) => {
            let _ = sqlx::query(
                "INSERT INTO bot_notification_delivery_log(kind,recipient_kind,recipient_id,status,detail) VALUES('manual_test','private',$1,'sent','Тест, запрошенный из Mini App')",
            )
            .bind(telegram_id)
            .execute(&state.db)
            .await;
            let _ = sqlx::query(
                "INSERT INTO bot_notification_worker_state(worker,last_success_at,last_error,updated_at) VALUES('telegram_reminders',now(),NULL,now()) ON CONFLICT(worker) DO UPDATE SET last_success_at=now(),last_error=NULL,updated_at=now()",
            )
            .execute(&state.db)
            .await;
            Ok(Json(json!({"sent":true,"message":"Тестовое сообщение отправлено в личный чат Telegram."})))
        }
        Err(error) => {
            let detail = error.to_string();
            let _ = sqlx::query(
                "INSERT INTO bot_notification_delivery_log(kind,recipient_kind,recipient_id,status,detail) VALUES('manual_test','private',$1,'failed',$2)",
            )
            .bind(telegram_id)
            .bind(detail.chars().take(500).collect::<String>())
            .execute(&state.db)
            .await;
            Err(AppError::Internal)
        }
    }
}
