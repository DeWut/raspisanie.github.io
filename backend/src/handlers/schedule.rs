use axum::{extract::{Path,Query,State},Json};
use serde::Deserialize; use serde_json::Value;
use crate::{auth::AuthUser,error::AppError,state::SharedState};
#[derive(Deserialize)] pub struct Range { pub from:Option<chrono::DateTime<chrono::Utc>>, pub to:Option<chrono::DateTime<chrono::Utc>> }
pub async fn group_schedule(State(s):State<SharedState>,_a:AuthUser,Path(group_id):Path<uuid::Uuid>,Query(q):Query<Range>)->Result<Json<Vec<Value>>,AppError>{
 let from=q.from.unwrap_or_else(||chrono::Utc::now()-chrono::Duration::days(7)); let to=q.to.unwrap_or_else(||chrono::Utc::now()+chrono::Duration::days(30));
 if to<=from || (to-from)>chrono::Duration::days(120){return Err(AppError::BadRequest)}
 let rows=sqlx::query("SELECT l.id,l.subject,l.lesson_type,l.starts_at,l.ends_at,l.week_day,l.subgroup,t.full_name AS teacher,r.name AS room,r.building FROM lessons l LEFT JOIN teachers t ON t.id=l.teacher_id LEFT JOIN rooms r ON r.id=l.room_id WHERE l.group_id=$1 AND l.starts_at >= $2 AND l.starts_at < $3 ORDER BY l.starts_at")
 .bind(group_id).bind(from).bind(to).fetch_all(&s.db).await?;
 use sqlx::Row; Ok(Json(rows.into_iter().map(|r|serde_json::json!({"id":r.get::<uuid::Uuid,_>("id"),"subject":r.get::<String,_>("subject"),"lesson_type":r.get::<Option<String>,_>("lesson_type"),"starts_at":r.get::<chrono::DateTime<chrono::Utc>,_>("starts_at"),"ends_at":r.get::<chrono::DateTime<chrono::Utc>,_>("ends_at"),"week_day":r.get::<Option<i16>,_>("week_day"),"subgroup":r.get::<Option<i16>,_>("subgroup"),"teacher":r.get::<Option<String>,_>("teacher"),"room":r.get::<Option<String>,_>("room"),"building":r.get::<Option<String>,_>("building")})).collect()))
}
