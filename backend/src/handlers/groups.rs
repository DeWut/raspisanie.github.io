use crate::{auth::AuthUser, error::AppError, state::SharedState};
use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::Value;
pub async fn list_groups(
    State(s): State<SharedState>,
    _a: AuthUser,
) -> Result<Json<Vec<Value>>, AppError> {
    let rows=sqlx::query("SELECT id,external_id,name,faculty,course,is_active FROM groups WHERE is_active=true ORDER BY name").fetch_all(&s.db).await?;
    use sqlx::Row;
    Ok(Json(rows.into_iter().map(|r|serde_json::json!({"id":r.get::<uuid::Uuid,_>("id"),"external_id":r.get::<String,_>("external_id"),"name":r.get::<String,_>("name"),"faculty":r.get::<Option<String>,_>("faculty"),"course":r.get::<Option<i16>,_>("course"),"is_active":r.get::<bool,_>("is_active")})).collect()))
}
pub async fn get_group(
    State(s): State<SharedState>,
    _a: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Value>, AppError> {
    let r =
        sqlx::query("SELECT id,external_id,name,faculty,course,is_active FROM groups WHERE id=$1")
            .bind(id)
            .fetch_optional(&s.db)
            .await?
            .ok_or(AppError::NotFound)?;
    use sqlx::Row;
    Ok(Json(
        serde_json::json!({"id":r.get::<uuid::Uuid,_>("id"),"external_id":r.get::<String,_>("external_id"),"name":r.get::<String,_>("name"),"faculty":r.get::<Option<String>,_>("faculty"),"course":r.get::<Option<i16>,_>("course"),"is_active":r.get::<bool,_>("is_active")}),
    ))
}
