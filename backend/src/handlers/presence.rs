use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::AppError, state::SharedState};

#[derive(Debug, Deserialize)]
pub struct PresencePing {
    pub session_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct PresenceCount {
    pub online: i64,
    pub window_seconds: i64,
}

// A random browser-generated session ID is stored; no IP address or Telegram profile is recorded.
pub async fn ping(
    State(state): State<SharedState>,
    Json(body): Json<PresencePing>,
) -> Result<StatusCode, AppError> {
    sqlx::query(
        "INSERT INTO online_presence (session_id, last_seen_at) VALUES ($1, now()) \
         ON CONFLICT (session_id) DO UPDATE SET last_seen_at = now()",
    )
    .bind(body.session_id)
    .execute(&state.db)
    .await?;

    // Opportunistic bounded cleanup keeps the table small without a separate worker.
    sqlx::query("DELETE FROM online_presence WHERE last_seen_at < now() - interval '1 day'")
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn count(State(state): State<SharedState>) -> Result<Json<PresenceCount>, AppError> {
    let online = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM online_presence WHERE last_seen_at >= now() - interval '2 minutes'",
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(PresenceCount {
        online,
        window_seconds: 120,
    }))
}
