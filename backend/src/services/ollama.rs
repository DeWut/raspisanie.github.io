use serde_json::{json, Value};
use std::time::Duration;

use crate::config::Config;

/// Optional local Ollama integration. Returns None on any unavailable/misconfigured
/// upstream so the caller can retain deterministic rule-based behavior.
pub async fn chat(config: &Config, prompt: &str, history: &[(String, String)]) -> Option<String> {
    let base_url = config.ollama_base_url.as_deref()?.trim_end_matches('/');
    let api_key = config.ollama_api_key.as_deref()?;
    if base_url.is_empty() || api_key.is_empty() {
        return None;
    }

    let mut messages = vec![json!({
        "role": "system",
        "content": "Ты Барни — учебный помощник СПбГЛТУ. Отвечай по-русски, естественно и кратко. Поддерживай контекст диалога. Помогай объяснять учебные темы, формулировать планы и уточнять запросы. Не выдумывай расписание, домашние задания, аудитории, преподавателей, дедлайны, статусы и данные пользователей: для точных данных используй только подключённые функции приложения; если данных нет, честно скажи об этом и предложи открыть соответствующий раздел. Не проси пароли, токены и персональные секреты. Если вопрос не относится к учёбе, ответь вежливо и верни разговор к возможностям помощника."
    })];

    for (role, content) in history.iter().rev().take(8).collect::<Vec<_>>().into_iter().rev() {
        if (role == "user" || role == "assistant") && !content.trim().is_empty() {
            messages.push(json!({"role": role, "content": content}));
        }
    }
    messages.push(json!({"role": "user", "content": prompt}));

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .ok()?;
    let response = client
        .post(format!("{base_url}/api/chat"))
        .bearer_auth(api_key)
        .json(&json!({
            "model": config.ollama_model,
            "messages": messages,
            "stream": false,
            "options": {"temperature": 0.4, "num_ctx": 4096}
        }))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        tracing::warn!("Local Ollama endpoint returned {}", response.status());
        return None;
    }
    let body = response.json::<Value>().await.ok()?;
    let answer = body.pointer("/message/content")?.as_str()?.trim();
    if answer.is_empty() {
        None
    } else {
        Some(answer.chars().take(4000).collect())
    }
}
