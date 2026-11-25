use std::collections::BTreeMap;

use axum::{async_trait, extract::FromRequestParts, http::request::Parts};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::{error::AppError, state::AppState};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramUser {
    pub id: i64,
    pub first_name: String,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub language_code: Option<String>,
    #[serde(default)]
    pub photo_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub telegram_id: i64,
    pub user: Option<TelegramUser>,
    pub is_dev_bypass: bool,
}

#[async_trait]
impl FromRequestParts<std::sync::Arc<AppState>> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &std::sync::Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        if state.config.dev_bypass_auth {
            let telegram_id = state.config.admin_telegram_id;
            if telegram_id <= 0 {
                return Err(AppError::Unauthorized);
            }
            return Ok(Self {
                telegram_id,
                user: None,
                is_dev_bypass: true,
            });
        }

        let init_data = parts
            .headers
            .get("x-telegram-init-data")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .ok_or(AppError::Unauthorized)?;

        let bot_token = state
            .config
            .teloxide_token
            .as_deref()
            .ok_or(AppError::Unauthorized)?;
        let fields = parse_init_data(init_data)?;
        validate_init_data(&fields, bot_token)?;

        let auth_date = fields
            .get("auth_date")
            .and_then(|value| value.parse::<i64>().ok())
            .ok_or(AppError::Unauthorized)?;
        let now = chrono::Utc::now().timestamp();
        if auth_date > now + 60 || now.saturating_sub(auth_date) > 86_400 {
            return Err(AppError::Unauthorized);
        }

        let raw_user = fields.get("user").ok_or(AppError::Unauthorized)?;
        let user: TelegramUser =
            serde_json::from_str(raw_user).map_err(|_| AppError::Unauthorized)?;
        if user.id <= 0 {
            return Err(AppError::Unauthorized);
        }

        Ok(Self {
            telegram_id: user.id,
            user: Some(user),
            is_dev_bypass: false,
        })
    }
}

fn parse_init_data(raw: &str) -> Result<BTreeMap<String, String>, AppError> {
    let mut fields = BTreeMap::new();
    for (key, value) in url::form_urlencoded::parse(raw.as_bytes()) {
        if fields
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            return Err(AppError::Unauthorized);
        }
    }
    if !fields.contains_key("hash") {
        return Err(AppError::Unauthorized);
    }
    Ok(fields)
}

fn validate_init_data(fields: &BTreeMap<String, String>, bot_token: &str) -> Result<(), AppError> {
    let supplied_hash = fields.get("hash").ok_or(AppError::Unauthorized)?;
    let supplied_hash = decode_hex(supplied_hash).ok_or(AppError::Unauthorized)?;

    let data_check_string = fields
        .iter()
        .filter(|(key, _)| key.as_str() != "hash")
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut secret_mac =
        HmacSha256::new_from_slice(b"WebAppData").map_err(|_| AppError::Internal)?;
    secret_mac.update(bot_token.as_bytes());
    let secret_key = secret_mac.finalize().into_bytes();

    let mut data_mac = HmacSha256::new_from_slice(&secret_key).map_err(|_| AppError::Internal)?;
    data_mac.update(data_check_string.as_bytes());
    let expected_hash = data_mac.finalize().into_bytes();

    if expected_hash
        .as_slice()
        .ct_eq(supplied_hash.as_slice())
        .unwrap_u8()
        != 1
    {
        return Err(AppError::Unauthorized);
    }
    Ok(())
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malformed_hash() {
        assert!(decode_hex("not-a-hash").is_none());
        assert!(decode_hex(&"a".repeat(64)).is_some());
    }
}
