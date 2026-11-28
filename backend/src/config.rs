use std::{env, num::ParseIntError};

use thiserror::Error;

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    pub rust_log: String,
    pub webapp_url: String,
    pub teloxide_token: Option<String>,
    pub telegram_webhook_secret: Option<String>,
    pub admin_telegram_id: i64,
    pub dev_bypass_auth: bool,
    pub database_url: String,
    pub openai_api_key: Option<String>,
    pub ollama_base_url: Option<String>,
    pub ollama_api_key: Option<String>,
    pub ollama_model: String,
    pub openai_base_url: String,
    pub embedding_model: String,
    pub embedding_dim: usize,
    pub cors_allowed_origins: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("environment variable {0} is required")]
    Missing(&'static str),
    #[error("invalid value for environment variable {0}")]
    Invalid(&'static str),
    #[error("could not read .env file: {0}")]
    Dotenv(#[from] dotenvy::Error),
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        match dotenvy::dotenv() {
            Ok(_) | Err(dotenvy::Error::Io(_)) => {}
            Err(error) => return Err(ConfigError::Dotenv(error)),
        }

        let port = parse_or("PORT", 8080_u16)?;
        let admin_telegram_id = parse_or("ADMIN_TELEGRAM_ID", 0_i64)?;
        let embedding_dim = parse_or("EMBEDDING_DIM", 1536_usize)?;
        if embedding_dim != 1536 {
            return Err(ConfigError::Invalid(
                "EMBEDDING_DIM (database uses vector(1536))",
            ));
        }

        let dev_bypass_auth = env_value("DEV_BYPASS_AUTH")
            .unwrap_or_else(|| "false".to_owned())
            .parse::<bool>()
            .map_err(|_| ConfigError::Invalid("DEV_BYPASS_AUTH"))?;

        let teloxide_token = optional("TELOXIDE_TOKEN");
        if !dev_bypass_auth && teloxide_token.is_none() {
            return Err(ConfigError::Missing("TELOXIDE_TOKEN"));
        }

        let cors_allowed_origins = env_value("CORS_ALLOWED_ORIGINS")
            .unwrap_or_else(|| "http://localhost:5173,https://dewut.github.io".to_owned())
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(str::to_owned)
            .collect();

        Ok(Self {
            port,
            rust_log: value_or("RUST_LOG", "info"),
            webapp_url: value_or("WEBAPP_URL", "http://localhost:5173"),
            teloxide_token,
            telegram_webhook_secret: optional("TELEGRAM_WEBHOOK_SECRET"),
            admin_telegram_id,
            dev_bypass_auth,
            database_url: required("DATABASE_URL")?,
            openai_api_key: optional("OPENAI_API_KEY"),
            ollama_base_url: optional("OLLAMA_BASE_URL"),
            ollama_api_key: optional("OLLAMA_API_KEY"),
            ollama_model: value_or("OLLAMA_MODEL", "lfm3"),
            openai_base_url: value_or("OPENAI_BASE_URL", "https://api.openai.com/v1"),
            embedding_model: value_or("EMBEDDING_MODEL", "text-embedding-3-small"),
            embedding_dim,
            cors_allowed_origins,
        })
    }
}

fn env_value(key: &'static str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn optional(key: &'static str) -> Option<String> {
    env_value(key).filter(|value| value != "replace_me")
}

fn required(key: &'static str) -> Result<String, ConfigError> {
    optional(key).ok_or(ConfigError::Missing(key))
}

fn value_or(key: &'static str, default: &str) -> String {
    env_value(key).unwrap_or_else(|| default.to_owned())
}

fn parse_or<T>(key: &'static str, default: T) -> Result<T, ConfigError>
where
    T: std::str::FromStr<Err = ParseIntError>,
{
    match env_value(key) {
        Some(value) => value.parse().map_err(|_| ConfigError::Invalid(key)),
        None => Ok(default),
    }
}
