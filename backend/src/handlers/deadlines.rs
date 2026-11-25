use axum::{extract::{Path,State},Json}; use serde::Deserialize; use serde_json::Value;
use crate::{auth::AuthUser,error::AppError,state::SharedState};
#[derive(Deserialize)] pub struct NewDeadline { pub external_id:String,pub course_name:String,pub title:String,pub description:Option<String>,pub due_at:chrono::DateTime<chrono::Utc>,pub source_url:Option<String> }
pub async fn list_deadlines(State(s):State<SharedState>,a:AuthUser)->Result<Json<Vec<Value>>,AppError>{
 let uid=super::user_id(&s,&a).await?; let rows=sqlx::query("SELECT id,external_id,course_name,title,description,due_at,source_url,completed FROM moodle_deadlines WHERE user_id=$1 ORDER BY completed,due_at").bind(uid).fetch_all(&s.db).await?; use sqlx::Row;
 Ok(Json(rows.into_iter().map(|r|serde_json::json!({"id":r.get::<uuid::Uuid,_>("id"),"external_id":r.get::<String,_>("external_id"),"course_name":r.get::<String,_>("course_name"),"title":r.get::<String,_>("title"),"description":r.get::<Option<String>,_>("description"),"due_at":r.get::<chrono::DateTime<chrono::Utc>,_>("due_at"),"source_url":r.get::<Option<String>,_>("source_url"),"completed":r.get::<bool,_>("completed")})).collect()))
}
pub async fn create_deadline(State(s):State<SharedState>,a:AuthUser,Json(v):Json<NewDeadline>)->Result<Json<Value>,AppError>{
 let uid=super::user_id(&s,&a).await?; let id:uuid::Uuid=sqlx::query_scalar("INSERT INTO moodle_deadlines(external_id,user_id,course_name,title,description,due_at,source_url) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(external_id) DO UPDATE SET course_name=EXCLUDED.course_name,title=EXCLUDED.title,description=EXCLUDED.description,due_at=EXCLUDED.due_at,source_url=EXCLUDED.source_url,updated_at=now() WHERE moodle_deadlines.user_id=$2 RETURNING id").bind(v.external_id).bind(uid).bind(v.course_name).bind(v.title).bind(v.description).bind(v.due_at).bind(v.source_url).fetch_optional(&s.db).await?.ok_or(AppError::Conflict)?; Ok(Json(serde_json::json!({"id":id})))
}
pub async fn complete_deadline(State(s):State<SharedState>,a:AuthUser,Path(id):Path<uuid::Uuid>)->Result<Json<Value>,AppError>{
 let uid=super::user_id(&s,&a).await?; let id:uuid::Uuid=sqlx::query_scalar("UPDATE moodle_deadlines SET completed=true,updated_at=now() WHERE id=$1 AND user_id=$2 RETURNING id").bind(id).bind(uid).fetch_optional(&s.db).await?.ok_or(AppError::NotFound)?; Ok(Json(serde_json::json!({"id":id,"completed":true})))
}
