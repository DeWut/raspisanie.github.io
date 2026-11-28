use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::http::{HeaderValue, Method};
use smart_campus_backend::{
    bot,
    config::Config,
    handlers,
    services::{eios_sync::EiosSync, schedule_sync},
    state::AppState,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use sqlx::postgres::PgPoolOptions;
use url::Url;

fn direct_neon_url(database_url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut parsed = Url::parse(database_url)?;
    if let Some(host) = parsed.host_str() {
        if host.contains("-pooler.") {
            let direct_host = host.replace("-pooler.", ".");
            parsed.set_host(Some(&direct_host))?;
        }
    }
    Ok(parsed.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;

    tracing_subscriber::fmt()
        .with_env_filter(&config.rust_log)
        .init();

    let state = AppState::new(config.clone()).await?;

    // Run SQLx migrations through Neon's direct endpoint. The app itself keeps
    // using DATABASE_URL (normally the pooled endpoint) for regular queries.
    let migration_url = match std::env::var("DATABASE_URL_UNPOOLED") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => direct_neon_url(&config.database_url)?,
    };
    let migration_pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .connect(&migration_url)
        .await?;
    sqlx::migrate!("./migrations").run(&migration_pool).await?;
    migration_pool.close().await;

    // Worker использует sync_metadata: запускается сразу только при отсутствии истории синхронизации,
    // а затем проверяет сохранённое время и обновляет расписание не чаще раза в 24 часа.
    match EiosSync::new("data/eios_cache", "1") {
        Ok(eios) => {
            let schedule_db = state.db.clone();

            tokio::spawn(async move {
                schedule_sync::run_daily_sync(schedule_db, eios).await;
            });
        }
        Err(error) => {
            tracing::error!("Could not initialize EIOS schedule synchronizer: {}", error);
        }
    }

    // Запускаем Telegram-бота.
    let bot_state: Arc<_> = state.clone();

    if config.teloxide_token.is_some() {
        let webhook_configured = config.telegram_webhook_secret.is_some()
            && (std::env::var("TELEGRAM_WEBHOOK_URL").is_ok()
                || std::env::var("RENDER_EXTERNAL_URL").is_ok());
        if webhook_configured {
            bot::start_webhook_mode(bot_state).await?;
        } else {
            tokio::spawn(async move {
                bot::run(bot_state).await;
            });
        }
    } else {
        tracing::warn!("Telegram bot token missing; bot disabled");
    }

    let origins = config
        .cors_allowed_origins
        .iter()
        .filter_map(|v| v.parse::<HeaderValue>().ok())
        .collect::<Vec<_>>();

    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any);

    let app = handlers::router()
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("SPBFTUIT API listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
