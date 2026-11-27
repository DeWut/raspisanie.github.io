use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

use crate::state::SharedState;

pub async fn health(
    State(state): State<SharedState>,
) -> (StatusCode, Json<Value>) {
    let database = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await;
    match database {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"status":"ok","service":"spbftuit-backend","database":"ok"})),
        ),
        Err(error) => {
            tracing::error!("Database health check failed: {}", error);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"status":"degraded","service":"spbftuit-backend","database":"unavailable"})),
            )
        }
    }
}
