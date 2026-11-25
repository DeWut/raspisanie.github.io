use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use reqwest::{Client, StatusCode};
use thiserror::Error;

use super::parser::{parse_group_html, parse_groups_html};
use crate::models::{Group, GroupSchedule};

const BASE_URL: &str = "https://eios.spbftu.ru";
const START_URL: &str = "https://eios.spbftu.ru/Rasp/";
const GROUP_URL: &str = "https://eios.spbftu.ru/Rasp/Rasp.aspx?group={group}&sem={semester}";

#[derive(Debug, Error)]
pub enum EiosSyncError {
    #[error("EIOS request failed: {0:?}")]
    Request(#[from] reqwest::Error),
    #[error("EIOS returned HTTP status {0}")]
    Http(StatusCode),
    #[error("could not read cached schedule: {0}")]
    Cache(#[from] std::io::Error),
    #[error("cached schedule JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct EiosSync {
    client: Client,
    cache_dir: PathBuf,
    semester: String,
    attempts: usize,
}
impl EiosSync {
    pub fn new(
        cache_dir: impl Into<PathBuf>,
        semester: impl Into<String>,
    ) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (compatible; SmartCampus/1.0)")
            .timeout(Duration::from_secs(25))
            .build()?;
        Ok(Self {
            client,
            cache_dir: cache_dir.into(),
            semester: semester.into(),
            attempts: 3,
        })
    }
    pub fn with_attempts(mut self, attempts: usize) -> Self {
        self.attempts = attempts.max(1);
        self
    }

    async fn get_text(&self, url: &str) -> Result<String, EiosSyncError> {
        let mut last_error = None;
        for attempt in 0..self.attempts {
            match self.client.get(url).send().await {
                Ok(response) if response.status().is_success() => {
                    return Ok(response.text().await?)
                }
                Ok(response) => {
                    let status = response.status();
                    if !(status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()) {
                        return Err(EiosSyncError::Http(status));
                    }
                    last_error = Some(EiosSyncError::Http(status));
                }
                Err(error) => last_error = Some(EiosSyncError::Request(error)),
            }
            if attempt + 1 < self.attempts {
                tokio::time::sleep(Duration::from_millis(
                    250_u64.saturating_mul(2_u64.pow(attempt.min(5) as u32)),
                ))
                .await;
            }
        }
        Err(last_error.unwrap_or_else(|| EiosSyncError::Http(StatusCode::BAD_GATEWAY)))
    }

    pub async fn discover_groups(&self) -> Result<Vec<Group>, EiosSyncError> {
        let html = self.get_text(START_URL).await?;
        Ok(parse_groups_html(&html))
    }

    /// Fetches the live EIOS page; on transient/unavailable EIOS, falls back to
    /// the repository's existing data/{id}.json schedule snapshot.
    pub async fn sync_group(&self, group_id: &str) -> Result<GroupSchedule, EiosSyncError> {
        let url = GROUP_URL
            .replace("{group}", group_id)
            .replace("{semester}", &self.semester);
        match self.get_text(&url).await {
            Ok(html) => Ok(parse_group_html(&html, group_id)),
            Err(network_error) => match self.read_cache(group_id).await {
                Ok(schedule) => Ok(schedule),
                Err(_) => Err(network_error),
            },
        }
    }

    async fn read_cache(&self, group_id: &str) -> Result<GroupSchedule, EiosSyncError> {
        let path = self.cache_dir.join(format!("{group_id}.json"));
        let raw = tokio::fs::read_to_string(path).await?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn base_url(&self) -> &'static str {
        BASE_URL
    }
    pub fn cache_path(&self, group_id: &str) -> impl AsRef<Path> {
        self.cache_dir.join(format!("{group_id}.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_uses_existing_eios_route() {
        assert!(GROUP_URL.contains("/Rasp/Rasp.aspx?group="));
    }
}
