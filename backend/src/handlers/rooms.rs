use axum::{extract::{Query,State},Json}; use serde::Deserialize; use serde_json::Value;
use crate::{auth::AuthUser,error::AppError,state::SharedState};
#[derive(Deserialize)] pub struct RoomQuery { pub q:Option<String>, pub building:Option<String> }
pub async fn list_rooms(State(s):State<SharedState>,_a:AuthUser,Query(q):Query<RoomQuery>)->Result<Json<Vec<Value>>,AppError>{
 let term=q.q.unwrap_or_default(); let rows=sqlx::query("SELECT id,external_id,name,building,capacity FROM rooms WHERE ($1='' OR name ILIKE '%'||$1||'%' OR external_id ILIKE '%'||$1||'%') AND ($2::text IS NULL OR building=$2) ORDER BY building,name LIMIT 200").bind(term).bind(q.building).fetch_all(&s.db).await?;
 use sqlx::Row; Ok(Json(rows.into_iter().map(|r|serde_json::json!({"id":r.get::<uuid::Uuid,_>("id"),"external_id":r.get::<String,_>("external_id"),"name":r.get::<String,_>("name"),"building":r.get::<Option<String>,_>("building"),"capacity":r.get::<Option<i32>,_>("capacity")})).collect()))
}
