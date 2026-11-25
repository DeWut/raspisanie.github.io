use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;

use crate::{config::Config, error::AppError};

pub struct AppState {
    pub config: Config,
    pub db: sqlx::PgPool,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub async fn new(config: Config) -> Result<SharedState, AppError> {
        let db = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(&config.database_url)
            .await?;

        Ok(Arc::new(Self { config, db }))
    }
}
