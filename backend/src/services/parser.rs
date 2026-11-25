use scraper::{Html, Selector};
use std::collections::BTreeMap;

use crate::models::{Group, GroupSchedule, Lesson};

const DAYS: [&str; 7] = ["понедельник", "вторник", "среда", "четверг", "пятница", "суббота", "воскресенье"];

fn clean(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parses the EIOS group list page and extracts group IDs from ?group= links.
pub fn parse_groups_html(html: &str) -> Vec<Group> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("a[href], option[value]").expect("static selector");
    let mut groups = BTreeMap::<String, String>::new();
    for node in document.select(&selector) {
        let (id, name) = if let Some(href) = node.value().attr("href") {
            let Some(query) = href.split('?').nth(1) else { continue };
            let Some(id) = query.split('&').find_map(|p| p.strip_prefix("group=")) else { continue };
            (id.to_owned(), clean(&node.text().collect::<Vec<_>>().join(" ")))
        } else {
            let Some(id) = node.value().attr("value").filter(|v| v.chars().all(|c| c.is_ascii_digit())) else { continue };
            (id.to_owned(), clean(&node.text().collect::<Vec<_>>().join(" ")))
        };
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
            groups.entry(id).and_modify(|old| if old.is_empty() && !name.is_empty() { *old = name.clone() })
                .or_insert(name);
        }
    }
    groups.into_iter().map(|(id, name)| Group { name: if name.is_empty() { id.clone() } else { name }, id }).collect()
}

/// Parses a group's timetable HTML into the same JSON shape as update_schedule.py.
/// EIOS has several table layouts; this parser intentionally uses row text heuristics
/// and returns an empty lesson map rather than fabricating lesson data when no table is found.
pub fn parse_group_html(html: &str, group_id: &str) -> GroupSchedule {
    let document = Html::parse_document(html);
    let title = Selector::parse("title").expect("static selector");
    let title_text = document.select(&title).next().map(|n| clean(&n.text().collect::<Vec<_>>().join(" "))).unwrap_or_default();
    let all_text = clean(&Html::parse_fragment(&document.root_element().text().collect::<Vec<_>>().join(" ")).root_element().text().collect::<Vec<_>>().join(" "));
    let name = find_group_name(&format!("{title_text} {all_text}")).unwrap_or_else(|| group_id.to_owned());
    let rows = Selector::parse("tr").expect("static selector");
    let cells = Selector::parse("td").expect("static selector");
    let mut lessons: BTreeMap<String, Vec<Lesson>> = BTreeMap::new();
    let mut day: Option<String> = None;
    let mut last_time: Option<(String, String, u8)> = None;
    for row in document.select(&rows) {
        let text = clean(&row.text().collect::<Vec<_>>().join(" "));
        let lower = text.to_lowercase();
        if let Some((index, _)) = DAYS.iter().enumerate().find(|(_, d)| lower.starts_with(**d)) {
            day = Some((index + 1).to_string());
            last_time = None;
            continue;
        }
        let Some(day_key) = day.clone() else { continue };
        let cell_text: Vec<String> = row.select(&cells).map(|c| clean(&c.text().collect::<Vec<_>>().join(" "))).filter(|s| !s.is_empty()).collect();
        if cell_text.is_empty() { continue; }
        let mut start_end = None;
        let mut pair = 0_u8;
        for cell in &cell_text {
            if let Some((s, e)) = parse_time_range(cell) {
                pair = pair_from_start(&s);
                start_end = Some((s, e, pair));
                break;
            }
        }
        if let Some(value) = start_end { last_time = Some(value); }
        let Some((start, end, pair)) = last_time.clone() else { continue };
        let week = if lower.contains("нечёт") || lower.contains("нечет") { "нечёт" }
            else if lower.contains("чёт") || lower.contains("чет") { "чёт" } else { "*" };
        let room_index = cell_text.iter().position(|v| v.to_lowercase().contains("ауд"));
        let room = room_index.map(|i| cell_text[i].clone()).unwrap_or_default();
        let teacher = room_index.and_then(|i| cell_text.get(i.wrapping_sub(1))).cloned().unwrap_or_default();
        let name = cell_text.iter().filter(|v| !v.contains(':') && !v.to_lowercase().contains("ауд") && !v.to_lowercase().contains("чёт") && !v.to_lowercase().contains("чет"))
            .max_by_key(|v| v.len()).cloned().unwrap_or_default();
        if name.is_empty() { continue; }
        lessons.entry(day_key).or_default().push(Lesson { pair, start, end, name, teacher, room, week: week.to_owned() });
    }
    GroupSchedule { id: group_id.to_owned(), name, updated: chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string(), lessons }
}

fn parse_time_range(s: &str) -> Option<(String, String)> {
    let bytes = s.as_bytes();
    for i in 0..bytes.len().saturating_sub(4) {
        if bytes[i].is_ascii_digit() {
            let tail = &s[i..];
            let mut it = tail.split(|c: char| !c.is_ascii_digit());
            let h1 = it.next()?; let m1 = it.next()?;
            if h1.len() > 2 || m1.len() != 2 { continue; }
            let rest = tail.find('-').or_else(|| tail.find('–')).map(|p| &tail[p + 1..]).or_else(|| tail.find(' ').map(|p| &tail[p + 1..]));
            if let Some(rest) = rest {
                let digits: String = rest.chars().filter(|c| c.is_ascii_digit() || *c == ':').collect();
                let mut p = digits.split(':');
                let h2 = p.next()?; let m2 = p.next()?;
                if !h2.is_empty() && m2.len() == 2 {
                    return Some((format!("{:02}:{:02}", h1.parse::<u8>().ok()?, m1.parse::<u8>().ok()?), format!("{:02}:{:02}", h2.parse::<u8>().ok()?, m2.parse::<u8>().ok()?)));
                }
            }
        }
    }
    None
}
fn pair_from_start(start: &str) -> u8 {
    match start { "09:15" => 1, "11:10" => 2, "13:30" => 3, "15:15" => 4, "17:00" => 5, _ => 0 }
}
fn find_group_name(text: &str) -> Option<String> {
    text.split_whitespace().find(|s| s.contains('-') && s.chars().any(|c| c.is_ascii_digit())).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_group_links_and_options() {
        let html = r#"<a href="/Rasp/Rasp.aspx?group=123&sem=1">ИТ-25-1</a><select><option value="124">Диз-25-1</option></select>"#;
        let found = parse_groups_html(html);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "123");
        assert_eq!(found[1].name, "Диз-25-1");
    }
    #[test]
    fn parses_a_minimal_schedule() {
        let html = r#"<title>ИТ-25-1</title><table><tr><td>Понедельник</td></tr><tr><td>09:15-10:50</td><td>Программирование</td><td>Иванов И.И.</td><td>ауд. 2.207</td></tr></table>"#;
        let parsed = parse_group_html(html, "123");
        assert_eq!(parsed.name, "ИТ-25-1");
        assert_eq!(parsed.lessons.get("1").unwrap()[0].name, "Программирование");
    }
}
