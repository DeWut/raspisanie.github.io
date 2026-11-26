use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

struct CalendarItem {
    uid: String,
    summary: String,
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
    location: Option<String>,
    description: Option<String>,
}

fn escape_ical_text(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

/// Folds a content line without splitting UTF-8 code points.
/// RFC 5545 limits physical content lines to 75 octets, including continuation space.
fn fold_ical_line(line: &str, output: &mut String) {
    let mut octets = 0usize;
    for character in line.chars() {
        let character_octets = character.len_utf8();
        if octets + character_octets > 75 {
            output.push_str("\r\n ");
            octets = 1;
        }
        output.push(character);
        octets += character_octets;
    }
    output.push_str("\r\n");
}

fn push_property(output: &mut String, name: &str, value: &str) {
    fold_ical_line(&format!("{name}:{value}"), output);
}

fn utc_timestamp(value: DateTime<Utc>) -> String {
    value.format("%Y%m%dT%H%M%SZ").to_string()
}

fn optional_description(parts: Vec<(&str, Option<String>)>) -> Option<String> {
    let lines: Vec<String> = parts
        .into_iter()
        .filter_map(|(label, value)| {
            value
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .map(|value| format!("{label}: {value}"))
        })
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

/// Builds an RFC 5545 calendar for the academic group configured in a Telegram chat.
/// Lesson end times come from the timetable. Exams and events intentionally have no
/// DTEND because their tables do not store a duration.
pub async fn build_group_calendar(
    db: &PgPool,
    chat_id: i64,
) -> Result<Option<String>, String> {
    let group_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT group_id FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE",
    )
    .bind(chat_id)
    .fetch_optional(db)
    .await
    .map_err(|error| format!("Не удалось получить настройки чата: {error}"))?
    .flatten();

    let Some(group_id) = group_id else {
        return Ok(None);
    };

    let mut items = Vec::new();

    let lessons = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            Option<String>,
            DateTime<Utc>,
            DateTime<Utc>,
            Option<String>,
            Option<String>,
        ),
    >(
        r#"
        SELECT l.id, l.subject, l.lesson_type, l.starts_at, l.ends_at,
               t.full_name, r.name
        FROM lessons l
        LEFT JOIN teachers t ON t.id=l.teacher_id
        LEFT JOIN rooms r ON r.id=l.room_id
        WHERE l.group_id=$1
          AND l.starts_at >= now()
          AND l.starts_at < now() + interval '180 days'
        ORDER BY l.starts_at
        LIMIT 2000
        "#,
    )
    .bind(group_id)
    .fetch_all(db)
    .await
    .map_err(|error| format!("Не удалось получить расписание: {error}"))?;

    for (id, subject, lesson_type, starts_at, ends_at, teacher, room) in lessons {
        items.push(CalendarItem {
            uid: format!("lesson-{id}@smart-campus"),
            summary: subject,
            starts_at,
            ends_at: Some(ends_at),
            location: room,
            description: optional_description(vec![
                ("Тип занятия", lesson_type),
                ("Преподаватель", teacher),
            ]),
        });
    }

    let exams = sqlx::query_as::<
        _,
        (
            i64,
            String,
            DateTime<Utc>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(
        r#"
        SELECT id, subject, exam_at, room, format, topics, literature, details
        FROM bot_exams
        WHERE chat_id=$1 AND status='active'
          AND exam_at >= now()
          AND exam_at < now() + interval '180 days'
        ORDER BY exam_at
        LIMIT 300
        "#,
    )
    .bind(chat_id)
    .fetch_all(db)
    .await
    .map_err(|error| format!("Не удалось получить экзамены: {error}"))?;

    for (id, subject, starts_at, room, format, topics, literature, details) in exams {
        items.push(CalendarItem {
            uid: format!("exam-{chat_id}-{id}@smart-campus"),
            summary: format!("Экзамен: {subject}"),
            starts_at,
            ends_at: None,
            location: room,
            description: optional_description(vec![
                ("Формат", format),
                ("Темы", topics),
                ("Литература", literature),
                ("Описание", details),
            ]),
        });
    }

    let events = sqlx::query_as::<
        _,
        (i64, String, DateTime<Utc>, Option<String>, Option<String>),
    >(
        r#"
        SELECT id, title, starts_at, location, description
        FROM bot_events
        WHERE chat_id=$1 AND status='active'
          AND starts_at >= now()
          AND starts_at < now() + interval '180 days'
        ORDER BY starts_at
        LIMIT 300
        "#,
    )
    .bind(chat_id)
    .fetch_all(db)
    .await
    .map_err(|error| format!("Не удалось получить мероприятия: {error}"))?;

    for (id, title, starts_at, location, description) in events {
        items.push(CalendarItem {
            uid: format!("event-{chat_id}-{id}@smart-campus"),
            summary: title,
            starts_at,
            ends_at: None,
            location,
            description,
        });
    }

    if items.is_empty() {
        return Ok(None);
    }

    items.sort_by_key(|item| item.starts_at);

    let now = Utc::now();
    let mut output = String::new();
    for line in [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//SPbFTU SMART CAMPUS//RU",
        "CALSCALE:GREGORIAN",
        "METHOD:PUBLISH",
        "X-WR-CALNAME:SPbFTU SMART CAMPUS",
    ] {
        fold_ical_line(line, &mut output);
    }

    for item in items {
        fold_ical_line("BEGIN:VEVENT", &mut output);
        push_property(&mut output, "UID", &escape_ical_text(&item.uid));
        push_property(&mut output, "DTSTAMP", &utc_timestamp(now));
        push_property(&mut output, "DTSTART", &utc_timestamp(item.starts_at));
        if let Some(ends_at) = item.ends_at {
            push_property(&mut output, "DTEND", &utc_timestamp(ends_at));
        }
        push_property(
            &mut output,
            "SUMMARY",
            &escape_ical_text(&item.summary),
        );
        if let Some(location) = item.location {
            if !location.trim().is_empty() {
                push_property(
                    &mut output,
                    "LOCATION",
                    &escape_ical_text(&location),
                );
            }
        }
        if let Some(description) = item.description {
            if !description.trim().is_empty() {
                push_property(
                    &mut output,
                    "DESCRIPTION",
                    &escape_ical_text(&description),
                );
            }
        }
        fold_ical_line("STATUS:CONFIRMED", &mut output);
        fold_ical_line("END:VEVENT", &mut output);
    }

    fold_ical_line("END:VCALENDAR", &mut output);
    Ok(Some(output))
}

#[cfg(test)]
mod tests {
    use super::{escape_ical_text, fold_ical_line};

    #[test]
    fn escapes_ical_text_values() {
        assert_eq!(
            escape_ical_text("A\\B;C,D\r\nE"),
            "A\\\\B\\;C\\,D\\nE"
        );
    }

    #[test]
    fn folds_lines_at_utf8_boundaries() {
        let line = format!("SUMMARY:{}", "расписание".repeat(20));
        let mut output = String::new();
        fold_ical_line(&line, &mut output);

        for physical_line in output.split("\r\n").filter(|line| !line.is_empty()) {
            assert!(physical_line.as_bytes().len() <= 75);
        }
        assert!(output.contains("\r\n "));
    }
}
