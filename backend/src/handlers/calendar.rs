use axum::{extract::{Query,State},Json}; use serde::Deserialize; use serde_json::Value;
use crate::{auth::AuthUser,error::AppError,state::SharedState};
#[derive(Deserialize)] pub struct CalendarQuery { pub from:Option<chrono::DateTime<chrono::Utc>>, pub to:Option<chrono::DateTime<chrono::Utc>> }
pub async fn calendar(State(s):State<SharedState>,a:AuthUser,Query(q):Query<CalendarQuery>)->Result<Json<Vec<Value>>,AppError>{
 let uid=super::user_id(&s,&a).await?; let from=q.from.unwrap_or_else(chrono::Utc::now); let to=q.to.unwrap_or_else(||from+chrono::Duration::days(90));
 if to<=from || (to-from)>chrono::Duration::days(180){return Err(AppError::BadRequest)}
 let rows=sqlx::query("SELECT d.id,d.course_name,d.title,d.description,d.due_at,d.source_url,d.completed FROM moodle_deadlines d WHERE d.user_id=$1 AND d.due_at >= $2 AND d.due_at < $3 ORDER BY d.due_at").bind(uid).bind(from).bind(to).fetch_all(&s.db).await?;
 use sqlx::Row; Ok(Json(rows.into_iter().map(|r|serde_json::json!({"id":r.get::<uuid::Uuid,_>("id"),"type":"deadline","course":r.get::<String,_>("course_name"),"title":r.get::<String,_>("title"),"description":r.get::<Option<String>,_>("description"),"starts_at":r.get::<chrono::DateTime<chrono::Utc>,_>("due_at"),"url":r.get::<Option<String>,_>("source_url"),"completed":r.get::<bool,_>("completed")})).collect()))
}
