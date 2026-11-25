use crate::{
    auth::AuthUser,
    error::AppError,
    services::rag::{CrawlConfig, RagService},
    state::SharedState,
};
use axum::{extract::State, Json};
use serde::Deserialize;
#[derive(Deserialize)]
pub struct ChatRequest {
    pub question: String,
    pub top_k: Option<i64>,
}
pub async fn chat(
    State(s): State<SharedState>,
    _a: AuthUser,
    Json(req): Json<ChatRequest>,
) -> Result<Json<crate::services::rag::ChatAnswer>, AppError> {
    let service = RagService::new(
        s.db.clone(),
        s.config.openai_api_key.clone(),
        s.config.openai_base_url.clone(),
        s.config.embedding_model.clone(),
        s.config.embedding_dim,
        "gpt-4o-mini",
        CrawlConfig::default(),
    )
    .map_err(|_| AppError::Internal)?;
    service
        .chat(&req.question, req.top_k.unwrap_or(5))
        .await
        .map(Json)
        .map_err(|_| AppError::Internal)
}
