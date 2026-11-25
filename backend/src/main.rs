use std::{net::SocketAddr, sync::Arc};
use axum::http::{HeaderValue, Method};
use smart_campus_backend::{bot, config::Config, handlers, state::AppState};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config=Config::from_env()?;
    tracing_subscriber::fmt().with_env_filter(&config.rust_log).init();
    let state=AppState::new(config.clone()).await?;
    sqlx::migrate!("./migrations").run(&state.db).await?;
    let bot_state: Arc<_>=state.clone();
    if config.teloxide_token.is_some() {
        tokio::spawn(async move { bot::run(bot_state).await; });
    } else {
        tracing::warn!("Telegram bot token missing; polling disabled");
    }
    let origins=config.cors_allowed_origins.iter().filter_map(|v|v.parse::<HeaderValue>().ok()).collect::<Vec<_>>();
    let cors=CorsLayer::new().allow_origin(origins).allow_methods([Method::GET,Method::POST,Method::PUT,Method::DELETE,Method::OPTIONS]).allow_headers(tower_http::cors::Any);
    let app=handlers::router().with_state(state).layer(cors).layer(TraceLayer::new_for_http());
    let addr=SocketAddr::from(([0,0,0,0],config.port));
    let listener=tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("SMART CAMPUS API listening on {}",addr);
    axum::serve(listener,app).await?;
    Ok(())
}
