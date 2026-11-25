use axum::{extract::{Path,State},Json}; use serde::Deserialize; use serde_json::Value;
use crate::{auth::AuthUser,error::AppError,state::SharedState};
#[derive(Deserialize)] pub struct NewSession { pub service_name:String,pub location:Option<String> }
pub async fn list_sessions(State(s):State<SharedState>,_a:AuthUser)->Result<Json<Vec<Value>>,AppError>{
 let rows=sqlx::query("SELECT id,service_name,location,status,opened_at,(SELECT count(*) FROM queue_entries e WHERE e.session_id=q.id AND e.status='waiting') AS waiting FROM queue_sessions q WHERE status='open' ORDER BY opened_at DESC").fetch_all(&s.db).await?;
 use sqlx::Row; Ok(Json(rows.into_iter().map(|r|serde_json::json!({"id":r.get::<uuid::Uuid,_>("id"),"service_name":r.get::<String,_>("service_name"),"location":r.get::<Option<String>,_>("location"),"status":r.get::<String,_>("status"),"opened_at":r.get::<chrono::DateTime<chrono::Utc>,_>("opened_at"),"waiting":r.get::<i64,_>("waiting")})).collect()))
}
pub async fn create_session(State(s):State<SharedState>,a:AuthUser,Json(v):Json<NewSession>)->Result<Json<Value>,AppError>{
 if v.service_name.trim().is_empty(){return Err(AppError::BadRequest)} let uid=super::user_id(&s,&a).await?; let id:uuid::Uuid=sqlx::query_scalar("INSERT INTO queue_sessions(service_name,location,created_by) VALUES($1,$2,$3) RETURNING id").bind(v.service_name.trim()).bind(v.location).bind(uid).fetch_one(&s.db).await?; Ok(Json(serde_json::json!({"id":id})))
}
pub async fn join_queue(State(s):State<SharedState>,a:AuthUser,Path(session):Path<uuid::Uuid>)->Result<Json<Value>,AppError>{
 let uid=super::user_id(&s,&a).await?; let mut tx=s.db.begin().await?; let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM queue_sessions WHERE id=$1 AND status='open')").bind(session).fetch_one(&mut *tx).await?; if !exists{return Err(AppError::NotFound)}
 let pos:i32=sqlx::query_scalar("SELECT COALESCE(MAX(position),0)+1 FROM queue_entries WHERE session_id=$1").bind(session).fetch_one(&mut *tx).await?;
 let id:uuid::Uuid=sqlx::query_scalar("INSERT INTO queue_entries(session_id,user_id,position) VALUES($1,$2,$3) ON CONFLICT(session_id,user_id) DO UPDATE SET status='waiting' RETURNING id").bind(session).bind(uid).bind(pos).fetch_one(&mut *tx).await?; tx.commit().await?; Ok(Json(serde_json::json!({"entry_id":id,"position":pos})))
}
