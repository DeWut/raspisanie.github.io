use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Domain representation of a group imported from the existing schedule feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
}

/// Lesson shape emitted by the repository's existing update_schedule.py parser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub pair: u8,
    pub start: String,
    pub end: String,
    pub name: String,
    #[serde(default)]
    pub teacher: String,
    #[serde(default)]
    pub room: String,
    #[serde(default = "default_week")]
    pub week: String,
}

fn default_week() -> String {
    "*".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupSchedule {
    pub id: String,
    pub name: String,
    pub updated: String,
    #[serde(default)]
    pub lessons: std::collections::BTreeMap<String, Vec<Lesson>>,
}

/// DTO for the Russian-labelled fields returned by EIOS exports.
/// Aliases accept title-cased labels without changing the canonical wire names.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EiosLessonDto {
    #[serde(rename = "дата", alias = "Дата", default)]
    pub date: Option<String>,
    #[serde(rename = "время", alias = "Время", default)]
    pub time: Option<String>,
    #[serde(rename = "дисциплина", alias = "Дисциплина", default)]
    pub subject: Option<String>,
    #[serde(rename = "преподаватель", alias = "Преподаватель", default)]
    pub teacher: Option<String>,
    #[serde(rename = "аудитория", alias = "Аудитория", default)]
    pub room: Option<String>,
    #[serde(rename = "группа", alias = "Группа", default)]
    pub group: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EiosScheduleDto {
    #[serde(default)]
    pub lessons: Vec<EiosLessonDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDto {
    pub id: Uuid,
    pub telegram_id: i64,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub created_at: DateTime<Utc>,
}
