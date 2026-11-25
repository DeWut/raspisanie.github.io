use axum::{extract::State, Json};
use serde_json::Value;
use crate::{auth::AuthUser, error::AppError, state::SharedState};
pub async fn get_me(State(state): State<SharedState>, auth: AuthUser) -> Result<Json<Value>, AppError> {
    let id = super::user_id(&state, &auth).await?;
    let row = sqlx::query("SELECT id, telegram_id, username, first_name, last_name, photo_url, group_id, role, settings, created_at FROM users WHERE id=$1")
        .bind(id).fetch_one(&state.db).await?;
    use sqlx::Row;
    Ok(Json(serde_json::json!({"id":row.try_get::<uuid::Uuid,_>("id")?,"telegram_id":row.try_get::<i64,_>("telegram_id")?,"username":row.try_get::<Option<String>,_>("username")?,"first_name":row.try_get::<Option<String>,_>("first_name")?,"last_name":row.try_get::<Option<String>,_>("last_name")?,"photo_url":row.try_get::<Option<String>,_>("photo_url")?,"group_id":row.try_get::<Option<uuid::Uuid>,_>("group_id")?,"role":row.try_get::<String,_>("role")?,"settings":row.try_get::<Value,_>("settings")?,"created_at":row.try_get::<chrono::DateTime<chrono::Utc>,_>("created_at")?})))
}
pub async fn update_me(State(state): State<SharedState>, auth: AuthUser, Json(body): Json<Value>) -> Result<Json<Value>, AppError> {
    let id=super::user_id(&state,&auth).await?;
    let group_id=body.get("group_id").and_then(Value::as_str).and_then(|s| uuid::Uuid::parse_str(s).ok());
    let settings=body.get("settings").cloned().unwrap_or_else(|| serde_json::json!({}));
    let row=sqlx::query("UPDATE users SET group_id=COALESCE($2,group_id), settings=$3, updated_at=now() WHERE id=$1 RETURNING id, group_id, settings")
        .bind(id).bind(group_id).bind(settings).fetch_one(&state.db).await?;
    use sqlx::Row;
    Ok(Json(serde_json::json!({"id":row.try_get::<uuid::Uuid,_>("id")?,"group_id":row.try_get::<Option<uuid::Uuid>,_>("group_id")?,"settings":row.try_get::<Value,_>("settings")?})))
}
