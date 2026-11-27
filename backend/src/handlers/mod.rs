pub mod calendar;
pub mod deadlines;
pub mod groups;
pub mod health;
pub mod help;
pub mod me;
pub mod presence;
pub mod queue;
pub mod rag;
pub mod rooms;
pub mod schedule;
pub mod teams;
pub mod tracker;

use crate::state::SharedState;
use axum::{
    routing::{get, post, put},
    Router,
};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/health", get(health::health))
        .route("/api/presence", get(presence::count))
        .route("/api/presence/ping", post(presence::ping))
        .route("/api/me", get(me::get_me).put(me::update_me))
        .route("/api/me/group", post(me::update_group))
        .route("/api/groups", get(groups::list_groups))
        .route("/api/groups/:group_id", get(groups::get_group))
        .route("/api/schedule/:group_id", get(schedule::group_schedule))
        .route("/api/rooms", get(rooms::list_rooms))
        .route("/api/calendar", get(calendar::calendar))
        .route("/api/rag/chat", post(rag::chat))
        .route(
            "/api/tracker/tasks",
            get(tracker::list_tasks).post(tracker::create_task),
        )
        .route(
            "/api/tracker/tasks/:task_id/progress",
            put(tracker::update_progress),
        )
        .route(
            "/api/teams",
            get(teams::list_projects).post(teams::create_project),
        )
        .route(
            "/api/teams/:project_id/applications",
            post(teams::apply_to_project),
        )
        .route(
            "/api/queue",
            get(queue::list_sessions).post(queue::create_session),
        )
        .route("/api/queue/:session_id/join", post(queue::join_queue))
        .route("/api/help", get(help::list_posts).post(help::create_post))
        .route("/api/help/:post_id", put(help::update_post))
        .route(
            "/api/deadlines",
            get(deadlines::list_deadlines).post(deadlines::create_deadline),
        )
        .route(
            "/api/deadlines/:deadline_id/complete",
            put(deadlines::complete_deadline),
        )
}

pub(crate) async fn user_id(
    state: &SharedState,
    auth: &crate::auth::AuthUser,
) -> Result<uuid::Uuid, crate::error::AppError> {
    let user_id = sqlx::query_scalar::<_, uuid::Uuid>(
        "INSERT INTO users (telegram_id, username, first_name, last_name, photo_url) VALUES ($1,$2,$3,$4,$5) \
         ON CONFLICT (telegram_id) DO UPDATE SET username=COALESCE(EXCLUDED.username, users.username), \
         first_name=COALESCE(EXCLUDED.first_name, users.first_name), last_name=COALESCE(EXCLUDED.last_name, users.last_name), \
         photo_url=COALESCE(EXCLUDED.photo_url, users.photo_url), updated_at=now() RETURNING id"
    )
    .bind(auth.telegram_id)
    .bind(auth.user.as_ref().and_then(|u| u.username.as_deref()))
    .bind(auth.user.as_ref().map(|u| u.first_name.as_str()))
    .bind(auth.user.as_ref().and_then(|u| u.last_name.as_deref()))
    .bind(auth.user.as_ref().and_then(|u| u.photo_url.as_deref()))
    .fetch_one(&state.db).await?;
    Ok(user_id)
}
