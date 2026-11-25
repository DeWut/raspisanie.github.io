use axum::{extract::{Path,State},Json}; use serde::Deserialize; use serde_json::Value;
use crate::{auth::AuthUser,error::AppError,state::SharedState};
#[derive(Deserialize)] pub struct NewTask { pub group_id:uuid::Uuid,pub title:String,pub description:Option<String>,pub due_at:Option<chrono::DateTime<chrono::Utc>> }
#[derive(Deserialize)] pub struct Progress { pub status:String }
pub async fn list_tasks(State(s):State<SharedState>,a:AuthUser)->Result<Json<Vec<Value>>,AppError>{
 let uid=super::user_id(&s,&a).await?; let rows=sqlx::query("SELECT t.id,t.group_id,t.title,t.description,t.due_at,COALESCE(p.status,'not_started') AS progress FROM group_tasks t JOIN users u ON u.group_id=t.group_id LEFT JOIN user_task_progress p ON p.task_id=t.id AND p.user_id=$1 WHERE u.id=$1 ORDER BY t.due_at NULLS LAST,t.created_at DESC").bind(uid).fetch_all(&s.db).await?;
 use sqlx::Row; Ok(Json(rows.into_iter().map(|r|serde_json::json!({"id":r.get::<uuid::Uuid,_>("id"),"group_id":r.get::<uuid::Uuid,_>("group_id"),"title":r.get::<String,_>("title"),"description":r.get::<Option<String>,_>("description"),"due_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("due_at"),"progress":r.get::<String,_>("progress")})).collect()))
}
pub async fn create_task(State(s):State<SharedState>,a:AuthUser,Json(v):Json<NewTask>)->Result<Json<Value>,AppError>{
 let uid=super::user_id(&s,&a).await?; let role:String=sqlx::query_scalar("SELECT role FROM users WHERE id=$1").bind(uid).fetch_one(&s.db).await?; if role!="admin" && role!="teacher"{return Err(AppError::Forbidden)}
 let r=sqlx::query("INSERT INTO group_tasks(group_id,title,description,due_at,created_by) VALUES($1,$2,$3,$4,$5) RETURNING id,created_at").bind(v.group_id).bind(v.title.trim()).bind(v.description).bind(v.due_at).bind(uid).fetch_one(&s.db).await?;
 use sqlx::Row; Ok(Json(serde_json::json!({"id":r.get::<uuid::Uuid,_>("id"),"created_at":r.get::<chrono::DateTime<chrono::Utc>,_>("created_at")})))
}
pub async fn update_progress(State(s):State<SharedState>,a:AuthUser,Path(task_id):Path<uuid::Uuid>,Json(v):Json<Progress>)->Result<Json<Value>,AppError>{
 if !["not_started","in_progress","completed"].contains(&v.status.as_str()){return Err(AppError::BadRequest)}
 let uid=super::user_id(&s,&a).await?; let r=sqlx::query("INSERT INTO user_task_progress(task_id,user_id,status,completed_at) VALUES($1,$2,$3,CASE WHEN $3='completed' THEN now() ELSE NULL END) ON CONFLICT(task_id,user_id) DO UPDATE SET status=EXCLUDED.status,completed_at=EXCLUDED.completed_at,updated_at=now() WHERE EXISTS(SELECT 1 FROM group_tasks t JOIN users u ON u.group_id=t.group_id WHERE t.id=$1 AND u.id=$2) RETURNING status").bind(task_id).bind(uid).bind(v.status).fetch_optional(&s.db).await?.ok_or(AppError::NotFound)?;
 use sqlx::Row; Ok(Json(serde_json::json!({"status":r.get::<String,_>("status")})))
}
