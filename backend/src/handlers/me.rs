use crate::{auth::AuthUser, error::AppError, state::SharedState};
use axum::{extract::State, Json};
use serde_json::Value;
pub async fn get_me(
    State(state): State<SharedState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let id = super::user_id(&state, &auth).await?;
    let row = sqlx::query("SELECT id, telegram_id, username, first_name, last_name, photo_url, group_id, role, settings, created_at FROM users WHERE id=$1")
        .bind(id).fetch_one(&state.db).await?;
    use sqlx::Row;
    Ok(Json(
        serde_json::json!({"id":row.try_get::<uuid::Uuid,_>("id")?,"telegram_id":row.try_get::<i64,_>("telegram_id")?,"username":row.try_get::<Option<String>,_>("username")?,"first_name":row.try_get::<Option<String>,_>("first_name")?,"last_name":row.try_get::<Option<String>,_>("last_name")?,"photo_url":row.try_get::<Option<String>,_>("photo_url")?,"group_id":row.try_get::<Option<uuid::Uuid>,_>("group_id")?,"role":row.try_get::<String,_>("role")?,"settings":row.try_get::<Value,_>("settings")?,"created_at":row.try_get::<chrono::DateTime<chrono::Utc>,_>("created_at")?}),
    ))
}
pub async fn update_me(
    State(state): State<SharedState>,
    auth: AuthUser,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let id = super::user_id(&state, &auth).await?;
    let group_id = body
        .get("group_id")
        .and_then(Value::as_str)
        .and_then(|s| uuid::Uuid::parse_str(s).ok());
    let settings = body
        .get("settings")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let row=sqlx::query("UPDATE users SET group_id=COALESCE($2,group_id), settings=$3, updated_at=now() WHERE id=$1 RETURNING id, group_id, settings")
        .bind(id).bind(group_id).bind(settings).fetch_one(&state.db).await?;
    use sqlx::Row;
    Ok(Json(
        serde_json::json!({"id":row.try_get::<uuid::Uuid,_>("id")?,"group_id":row.try_get::<Option<uuid::Uuid>,_>("group_id")?,"settings":row.try_get::<Value,_>("settings")?}),
    ))
}


/// Binds the Telegram-authenticated Mini App user to an active EIOS group.
/// The external EIOS ID is used because the static Mini App schedule catalogue
/// is built from data/groups.json and does not expose the database UUID.
pub async fn update_group(
    State(state): State<SharedState>,
    auth: AuthUser,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let external_id = body
        .get("external_id")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 32
                && value.bytes().all(|byte| byte.is_ascii_digit())
        })
        .ok_or(AppError::BadRequest)?;

    let user_id = super::user_id(&state, &auth).await?;
    let group = sqlx::query_as::<_, (uuid::Uuid, String)>(
        "SELECT id, name FROM groups WHERE external_id=$1 AND is_active=TRUE",
    )
    .bind(external_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    sqlx::query("UPDATE users SET group_id=$2,updated_at=now() WHERE id=$1")
        .bind(user_id)
        .bind(group.0)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({
        "group_id": group.0,
        "group_name": group.1,
        "external_id": external_id,
    })))
}
