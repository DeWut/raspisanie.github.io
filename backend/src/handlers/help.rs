use axum::{extract::{Path,State},Json}; use serde::Deserialize; use serde_json::Value;
use crate::{auth::AuthUser,error::AppError,state::SharedState};
#[derive(Deserialize)] pub struct NewPost { pub title:String,pub body:String,pub category:Option<String>,pub is_anonymous:Option<bool> }
#[derive(Deserialize)] pub struct EditPost { pub status:String }
pub async fn list_posts(State(s):State<SharedState>,_a:AuthUser)->Result<Json<Vec<Value>>,AppError>{
 let rows=sqlx::query("SELECT id,title,body,category,status,is_anonymous,created_at FROM help_board WHERE status='open' ORDER BY created_at DESC LIMIT 100").fetch_all(&s.db).await?; use sqlx::Row;
 Ok(Json(rows.into_iter().map(|r|serde_json::json!({"id":r.get::<uuid::Uuid,_>("id"),"title":r.get::<String,_>("title"),"body":r.get::<String,_>("body"),"category":r.get::<String,_>("category"),"status":r.get::<String,_>("status"),"is_anonymous":r.get::<bool,_>("is_anonymous"),"created_at":r.get::<chrono::DateTime<chrono::Utc>,_>("created_at")})).collect()))
}
pub async fn create_post(State(s):State<SharedState>,a:AuthUser,Json(v):Json<NewPost>)->Result<Json<Value>,AppError>{
 if v.title.trim().is_empty()||v.body.trim().is_empty()||v.title.len()>160||v.body.len()>10000{return Err(AppError::BadRequest)} let uid=super::user_id(&s,&a).await?;
 let id:uuid::Uuid=sqlx::query_scalar("INSERT INTO help_board(author_id,title,body,category,is_anonymous) VALUES($1,$2,$3,$4,$5) RETURNING id").bind(uid).bind(v.title.trim()).bind(v.body.trim()).bind(v.category.unwrap_or_else(||"general".into())).bind(v.is_anonymous.unwrap_or(false)).fetch_one(&s.db).await?; Ok(Json(serde_json::json!({"id":id})))
}
pub async fn update_post(State(s):State<SharedState>,a:AuthUser,Path(post):Path<uuid::Uuid>,Json(v):Json<EditPost>)->Result<Json<Value>,AppError>{
 if !["open","closed","resolved"].contains(&v.status.as_str()){return Err(AppError::BadRequest)} let uid=super::user_id(&s,&a).await?;
 let id:uuid::Uuid=sqlx::query_scalar("UPDATE help_board SET status=$3,updated_at=now() WHERE id=$1 AND author_id=$2 RETURNING id").bind(post).bind(uid).bind(v.status).fetch_optional(&s.db).await?.ok_or(AppError::NotFound)?; Ok(Json(serde_json::json!({"id":id})))
}
