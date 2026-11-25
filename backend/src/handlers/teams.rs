use axum::{extract::{Path,State},Json}; use serde::Deserialize; use serde_json::Value;
use crate::{auth::AuthUser,error::AppError,state::SharedState};
#[derive(Deserialize)] pub struct NewProject { pub title:String,pub description:Option<String>,pub tags:Option<Vec<String>> }
#[derive(Deserialize)] pub struct Application { pub message:Option<String> }
pub async fn list_projects(State(s):State<SharedState>,_a:AuthUser)->Result<Json<Vec<Value>>,AppError>{
 let rows=sqlx::query("SELECT p.id,p.title,p.description,p.status,p.tags,p.created_at,u.first_name AS owner FROM team_projects p JOIN users u ON u.id=p.owner_id WHERE p.status='open' ORDER BY p.created_at DESC LIMIT 100").fetch_all(&s.db).await?;
 use sqlx::Row; Ok(Json(rows.into_iter().map(|r|serde_json::json!({"id":r.get::<uuid::Uuid,_>("id"),"title":r.get::<String,_>("title"),"description":r.get::<Option<String>,_>("description"),"status":r.get::<String,_>("status"),"tags":r.get::<Vec<String>,_>("tags"),"created_at":r.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),"owner":r.get::<Option<String>,_>("owner")})).collect()))
}
pub async fn create_project(State(s):State<SharedState>,a:AuthUser,Json(v):Json<NewProject>)->Result<Json<Value>,AppError>{
 if v.title.trim().is_empty() || v.title.len()>160{return Err(AppError::BadRequest)} let uid=super::user_id(&s,&a).await?;
 let id:uuid::Uuid=sqlx::query_scalar("INSERT INTO team_projects(owner_id,title,description,tags) VALUES($1,$2,$3,$4) RETURNING id").bind(uid).bind(v.title.trim()).bind(v.description).bind(v.tags.unwrap_or_default()).fetch_one(&s.db).await?;
 Ok(Json(serde_json::json!({"id":id})))
}
pub async fn apply_to_project(State(s):State<SharedState>,a:AuthUser,Path(project):Path<uuid::Uuid>,Json(v):Json<Application>)->Result<Json<Value>,AppError>{
 let uid=super::user_id(&s,&a).await?; let id:uuid::Uuid=sqlx::query_scalar("INSERT INTO team_applications(project_id,applicant_id,message) SELECT id,$2,$3 FROM team_projects WHERE id=$1 AND status='open' AND owner_id<>$2 RETURNING id").bind(project).bind(uid).bind(v.message).fetch_optional(&s.db).await?.ok_or(AppError::NotFound)?;
 Ok(Json(serde_json::json!({"application_id":id,"status":"pending"})))
}
