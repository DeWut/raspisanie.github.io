use std::{collections::HashSet, time::Duration};

use reqwest::{redirect::Policy, Client, Url};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_CHUNKS_PER_DOCUMENT: usize = 500;
const DEFAULT_CHUNK_CHARS: usize = 1_200;
const DEFAULT_CHUNK_OVERLAP: usize = 160;

#[derive(Debug, Error)]
pub enum RagError {
    #[error("invalid or disallowed URL")]
    InvalidUrl,
    #[error("page is too large")]
    PageTooLarge,
    #[error("unsupported page content type")]
    UnsupportedContentType,
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("database operation failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("OpenAI-compatible API returned an error: {0}")]
    Api(String),
    #[error("invalid API response: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct CrawlConfig {
    /// Exact host allow-list. Redirects are disabled to prevent redirect-based SSRF.
    pub allowed_hosts: HashSet<String>,
    pub max_page_bytes: usize,
    pub chunk_chars: usize,
    pub chunk_overlap: usize,
}
impl Default for CrawlConfig {
    fn default() -> Self {
        Self {
            allowed_hosts: HashSet::new(),
            max_page_bytes: MAX_PAGE_BYTES,
            chunk_chars: DEFAULT_CHUNK_CHARS,
            chunk_overlap: DEFAULT_CHUNK_OVERLAP,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawledDocument {
    pub title: String,
    pub source_url: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagSource {
    pub document_id: Uuid,
    pub title: String,
    pub source: Option<String>,
    pub content: String,
    pub similarity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatAnswer {
    pub answer: String,
    pub sources: Vec<RagSource>,
}

#[derive(Clone)]
pub struct RagService {
    db: PgPool,
    http: Client,
    api_key: Option<String>,
    api_base: String,
    embedding_model: String,
    embedding_dim: usize,
    chat_model: String,
    crawl: CrawlConfig,
}

impl RagService {
    pub fn new(
        db: PgPool,
        api_key: Option<String>,
        api_base: impl Into<String>,
        embedding_model: impl Into<String>,
        embedding_dim: usize,
        chat_model: impl Into<String>,
        crawl: CrawlConfig,
    ) -> Result<Self, RagError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(45))
            .redirect(Policy::none())
            .user_agent("SPbFTU-SmartCampus-RAG/1.0")
            .build()?;
        Ok(Self {
            db,
            http,
            api_key,
            api_base: api_base.into().trim_end_matches('/').to_owned(),
            embedding_model: embedding_model.into(),
            embedding_dim,
            chat_model: chat_model.into(),
            crawl,
        })
    }

    /// Fetches one explicitly allowed HTML page. This is deliberately not a
    /// recursive crawler: callers control the seed URLs and host allow-list.
    pub async fn crawl_url(&self, raw_url: &str) -> Result<CrawledDocument, RagError> {
        let url = Url::parse(raw_url).map_err(|_| RagError::InvalidUrl)?;
        if url.scheme() != "https" && url.scheme() != "http" {
            return Err(RagError::InvalidUrl);
        }
        let host = url
            .host_str()
            .ok_or(RagError::InvalidUrl)?
            .to_ascii_lowercase();
        if !self.crawl.allowed_hosts.contains(&host)
            || url.username() != ""
            || url.password().is_some()
            || host == "localhost"
            || host.ends_with(".localhost")
        {
            return Err(RagError::InvalidUrl);
        }

        let response = self.http.get(url.clone()).send().await?;
        if !response.status().is_success() {
            return Err(RagError::Api(format!(
                "crawler HTTP status {}",
                response.status()
            )));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !content_type.to_ascii_lowercase().contains("text/html") {
            return Err(RagError::UnsupportedContentType);
        }
        if response
            .content_length()
            .is_some_and(|n| n as usize > self.crawl.max_page_bytes)
        {
            return Err(RagError::PageTooLarge);
        }
        let bytes = response.bytes().await?;
        if bytes.len() > self.crawl.max_page_bytes {
            return Err(RagError::PageTooLarge);
        }
        let html = String::from_utf8_lossy(&bytes);
        let document = scraper::Html::parse_document(&html);
        let title_selector = scraper::Selector::parse("title").expect("static selector");
        let title = document
            .select(&title_selector)
            .next()
            .map(|n| n.text().collect::<Vec<_>>().join(" "))
            .map(|s| normalize(&s))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| url.host_str().unwrap_or("Web page").to_owned());

        let body_selector = scraper::Selector::parse("body").expect("static selector");
        let content = document
            .select(&body_selector)
            .next()
            .map(|body| {
                let mut text = Vec::new();
                for node in body.text() {
                    let part = normalize(node);
                    if !part.is_empty() {
                        text.push(part);
                    }
                }
                text.join(" ")
            })
            .unwrap_or_default();
        if content.trim().is_empty() {
            return Err(RagError::Api(
                "HTML page contains no readable body text".into(),
            ));
        }
        Ok(CrawledDocument {
            title,
            source_url: url.to_string(),
            content,
        })
    }

    /// Crawls, chunks, embeds and atomically replaces the stored chunks for a URL.
    pub async fn index_url(&self, raw_url: &str) -> Result<Uuid, RagError> {
        let doc = self.crawl_url(raw_url).await?;
        self.index_document(&doc).await
    }

    pub async fn index_document(&self, doc: &CrawledDocument) -> Result<Uuid, RagError> {
        let chunks = chunk_text(
            &doc.content,
            self.crawl.chunk_chars,
            self.crawl.chunk_overlap,
        );
        if chunks.is_empty() || chunks.len() > MAX_CHUNKS_PER_DOCUMENT {
            return Err(RagError::Api(
                "document has no text or exceeds chunk limit".into(),
            ));
        }

        let mut embedded = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            embedded.push(self.embed(chunk).await?);
        }

        let mut tx = self.db.begin().await?;
        let document_id: Uuid = sqlx::query_scalar(
            "INSERT INTO rag_documents (external_id, title, source, content_type, metadata, updated_at) \
             VALUES ($1, $2, $1, 'text/html', jsonb_build_object('indexed_by','rag_service'), now()) \
             ON CONFLICT (external_id) DO UPDATE SET title=EXCLUDED.title, source=EXCLUDED.source, \
             content_type=EXCLUDED.content_type, metadata=EXCLUDED.metadata, updated_at=now() RETURNING id"
        )
        .bind(&doc.source_url)
        .bind(&doc.title)
        .fetch_one(&mut *tx).await?;

        sqlx::query("DELETE FROM rag_chunks WHERE document_id = $1")
            .bind(document_id)
            .execute(&mut *tx)
            .await?;

        for (index, (chunk, vector)) in chunks.iter().zip(embedded.iter()).enumerate() {
            let vector_literal = vector_to_pg(vector);
            sqlx::query(
                "INSERT INTO rag_chunks (document_id, chunk_index, content, embedding, metadata) \
                 VALUES ($1, $2, $3, $4::vector, jsonb_build_object('source',$5))",
            )
            .bind(document_id)
            .bind(index as i32)
            .bind(chunk)
            .bind(vector_literal)
            .bind(&doc.source_url)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(document_id)
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, RagError> {
        let key = self
            .api_key
            .as_deref()
            .ok_or_else(|| RagError::Api("OPENAI_API_KEY is not configured".into()))?;
        #[derive(Serialize)]
        struct Request<'a> {
            model: &'a str,
            input: &'a str,
        }
        #[derive(Deserialize)]
        struct Response {
            data: Vec<EmbeddingItem>,
        }
        #[derive(Deserialize)]
        struct EmbeddingItem {
            embedding: Vec<f32>,
        }

        let response = self
            .http
            .post(format!("{}/embeddings", self.api_base))
            .bearer_auth(key)
            .json(&Request {
                model: &self.embedding_model,
                input: text,
            })
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(RagError::Api(format!(
                "embeddings HTTP {status}: {}",
                truncate(&body, 500)
            )));
        }
        let parsed: Response = response.json().await?;
        let vector = parsed
            .data
            .into_iter()
            .next()
            .ok_or_else(|| RagError::Api("embeddings response has no data".into()))?
            .embedding;
        if vector.len() != self.embedding_dim || vector.iter().any(|v| !v.is_finite()) {
            return Err(RagError::Api(format!(
                "embedding dimension mismatch; expected {}",
                self.embedding_dim
            )));
        }
        Ok(vector)
    }

    /// Retrieval-augmented chat: vector-search relevant chunks and pass only
    /// those excerpts to the configured OpenAI-compatible chat-completions API.
    pub async fn chat(&self, question: &str, top_k: i64) -> Result<ChatAnswer, RagError> {
        let question = question.trim();
        if question.is_empty() || question.len() > 8_000 {
            return Err(RagError::Api(
                "question must contain 1..8000 characters".into(),
            ));
        }
        let query_vector = vector_to_pg(&self.embed(question).await?);
        let rows = sqlx::query_as::<_, (Uuid, String, Option<String>, String, f32)>(
            "SELECT d.id, d.title, d.source, c.content, \
             (1 - (c.embedding <=> $1::vector))::real AS similarity \
             FROM rag_chunks c JOIN rag_documents d ON d.id=c.document_id \
             WHERE c.embedding IS NOT NULL ORDER BY c.embedding <=> $1::vector LIMIT $2",
        )
        .bind(query_vector)
        .bind(top_k.clamp(1, 8))
        .fetch_all(&self.db)
        .await?;

        let sources: Vec<RagSource> = rows
            .into_iter()
            .map(
                |(document_id, title, source, content, similarity)| RagSource {
                    document_id,
                    title,
                    source,
                    content,
                    similarity,
                },
            )
            .collect();
        if sources.is_empty() {
            return Ok(ChatAnswer {
                answer: "В базе знаний пока нет подходящих материалов.".into(),
                sources,
            });
        }

        #[derive(Serialize)]
        struct Message<'a> {
            role: &'a str,
            content: String,
        }
        #[derive(Serialize)]
        struct ChatRequest<'a> {
            model: &'a str,
            temperature: f32,
            messages: Vec<Message<'a>>,
        }
        #[derive(Deserialize)]
        struct ChatResponse {
            choices: Vec<Choice>,
        }
        #[derive(Deserialize)]
        struct Choice {
            message: ChatMessage,
        }
        #[derive(Deserialize)]
        struct ChatMessage {
            content: String,
        }

        let context = sources
            .iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    "[Источник {}: {} | {}]\n{}",
                    i + 1,
                    s.title,
                    s.source.as_deref().unwrap_or(""),
                    s.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        let body = ChatRequest {
            model: &self.chat_model, temperature: 0.2,
            messages: vec![
                Message { role: "system", content: "Ты помощник СПбГТИ(ТУ). Отвечай по-русски, опирайся на предоставленные источники. Если ответа в источниках нет, прямо скажи об этом. Не выдумывай факты.".into() },
                Message { role: "user", content: format!("Материалы:\n{context}\n\nВопрос: {question}") },
            ],
        };
        let key = self
            .api_key
            .as_deref()
            .ok_or_else(|| RagError::Api("OPENAI_API_KEY is not configured".into()))?;
        let response = self
            .http
            .post(format!("{}/chat/completions", self.api_base))
            .bearer_auth(key)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(RagError::Api(format!(
                "chat HTTP {status}: {}",
                truncate(&body, 500)
            )));
        }
        let parsed: ChatResponse = response.json().await?;
        let answer = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| RagError::Api("chat response has no answer".into()))?;
        Ok(ChatAnswer { answer, sources })
    }
}

fn normalize(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Character-boundary-aware chunker with configurable overlap.
pub fn chunk_text(input: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    let overlap = overlap.min(max_chars.saturating_sub(1));
    let text = normalize(input);
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() && chunks.len() < MAX_CHUNKS_PER_DOCUMENT {
        let hard_end = (start + max_chars).min(chars.len());
        let mut end = hard_end;
        if hard_end < chars.len() {
            if let Some(relative) = chars[start..hard_end]
                .iter()
                .rposition(|c| c.is_whitespace())
            {
                if relative > max_chars / 2 {
                    end = start + relative;
                }
            }
        }
        if end <= start {
            end = hard_end;
        }
        let chunk: String = chars[start..end]
            .iter()
            .collect::<String>()
            .trim()
            .to_owned();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
        if end == chars.len() {
            break;
        }
        start = end.saturating_sub(overlap);
        if start == end {
            start += 1;
        }
    }
    chunks
}

fn vector_to_pg(vector: &[f32]) -> String {
    format!(
        "[{}]",
        vector
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunker_keeps_unicode_and_respects_limit() {
        let chunks = chunk_text("Привет мир. Это тестовый текст для разбиения.", 18, 3);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| c.chars().count() <= 18));
        assert!(chunks.join(" ").contains("Привет"));
    }

    #[test]
    fn chunker_handles_empty_and_short_text() {
        assert!(chunk_text("   ", 100, 10).is_empty());
        assert_eq!(chunk_text("коротко", 100, 10), vec!["коротко"]);
    }
}
