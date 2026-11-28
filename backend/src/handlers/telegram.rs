use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use teloxide::types::Update;

use crate::{bot, state::SharedState};

/// Telegram validates this secret header for every webhook delivery.
pub async fn telegram_webhook(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(update): Json<Update>,
) -> StatusCode {
    let Some(expected) = state.config.telegram_webhook_secret.as_deref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let supplied = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    use subtle::ConstantTimeEq;
    if supplied.len() != expected.len()
        || !bool::from(supplied.as_bytes().ct_eq(expected.as_bytes()))
    {
        return StatusCode::UNAUTHORIZED;
    }

    match bot::handle_webhook_update(state, update).await {
        Ok(()) => StatusCode::OK,
        Err(error) => {
            tracing::error!("Telegram webhook update failed: {}", error);
            // A non-2xx response lets Telegram retry delivery.
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
