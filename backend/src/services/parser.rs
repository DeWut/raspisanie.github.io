use scraper::{Html, Selector};
use std::collections::BTreeMap;

use crate::models::{Group, GroupSchedule, Lesson};

const DAYS: [&str; 7] = [
    "понедельник",
    "вторник",
    "среда",
    "четверг",
    "пятница",
    "суббота",
    "воскресенье",
];

fn clean(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn lower(value: &str) -> String {
    value.to_lowercase().replace('ё', "е")
}

fn is_group_label(label: &str, id: &str) -> bool {
    !label.is_empty()
        && label != id
        && label.len() <= 60
        && label.contains('-')
        && label.chars().any(|c| c.is_ascii_digit())
}

/// Извлекает группы из ссылок и элементов option.
pub fn parse_groups_html(html: &str) -> Vec<Group> {
    let document = Html::parse_document(html);
    let links = Selector::parse("a[href]").expect("static selector");
    let options = Selector::parse("option[value]").expect("static selector");

    let mut groups = BTreeMap::<String, String>::new();

    for node in document.select(&links) {
        let Some(href) = node.value().attr("href") else {
            continue;
        };

        let Some(query) = href.split('?').nth(1) else {
            continue;
        };

        let Some(id) = query
            .split('&')
            .find_map(|part| part.strip_prefix("group="))
        else {
            continue;
        };

        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let name = clean(&node.text().collect::<Vec<_>>().join(" "));

        if is_group_label(&name, id) {
            groups.insert(id.to_owned(), name);
        }
    }

    for node in document.select(&options) {
        let Some(id) = node.value().attr("value") else {
            continue;
        };

        if !id.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let name = clean(&node.text().collect::<Vec<_>>().join(" "));

        if is_group_label(&name, id) {
            groups.entry(id.to_owned()).or_insert(name);
        }
    }

    groups
        .into_iter()
        .map(|(id, name)| Group { id, name })
        .collect()
}

/// Разбивает содержимое ячейки на строки, сохраняя переносы br.
fn cell_lines(cell: scraper::ElementRef<'_>) -> Vec<String> {
    let mut html = cell.html();

    html = html
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<BR>", "\n")
        .replace("<BR/>", "\n")
        .replace("<BR />", "\n");

    let fragment = Html::parse_fragment(&html);
    let text = fragment.root_element().text().collect::<Vec<_>>().join(" ");

    let lines: Vec<String> = text
        .split('\n')
        .map(clean)
        .filter(|line| !line.is_empty())
        .collect();

    if lines.is_empty() {
        vec![clean(&cell.text().collect::<Vec<_>>().join(" "))]
    } else {
        lines
    }
}

fn find_day(text: &str) -> Option<String> {
    let normalized = lower(text);

    DAYS.iter()
        .enumerate()
        .find(|(_, day)| normalized.trim().starts_with(**day))
        .map(|(index, _)| (index + 1).to_string())
}

/// Возвращает пару, время начала и время окончания.
fn parse_time_cell(text: &str) -> Option<(u8, String, String)> {
    let normalized = lower(text);

    let pair = normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .find_map(|parts| {
            if parts[1].starts_with("пара") {
                parts[0].parse::<u8>().ok()
            } else {
                None
            }
        });

    let mut times = Vec::new();
    let chars: Vec<char> = normalized.chars().collect();

    let mut i = 0;

    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }

        let start = i;

        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }

        if i >= chars.len() || chars[i] != ':' {
            continue;
        }

        i += 1;

        let minute_start = i;

        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }

        if i - minute_start != 2 {
            continue;
        }

        let hour: String = chars[start..minute_start - 1].iter().collect();
        let minute: String = chars[minute_start..i].iter().collect();

        let Ok(hour_num) = hour.parse::<u8>() else {
            continue;
        };

        let Ok(minute_num) = minute.parse::<u8>() else {
            continue;
        };

        if hour_num > 23 || minute_num > 59 {
            continue;
        }

        times.push(format!("{:02}:{:02}", hour_num, minute_num));
    }

    if times.len() < 2 {
        return None;
    }

    let start = times[0].clone();
    let end = times[1].clone();

    let pair = pair.unwrap_or_else(|| pair_from_start(&start));

    Some((pair, start, end))
}

fn pair_from_start(start: &str) -> u8 {
    match start {
        "09:15" => 1,
        "11:10" => 2,
        "13:30" => 3,
        "15:15" => 4,
        "17:00" => 5,
        _ => 0,
    }
}

fn week_from_text(text: &str) -> &'static str {
    let normalized = lower(text);

    if normalized.contains("нечет") {
        "нечёт"
    } else if normalized.contains("чет") {
        "чёт"
    } else {
        "*"
    }
}

fn split_teacher_room(text: &str) -> (String, String) {
    let normalized = clean(text);
    let lower_text = lower(&normalized);

    if let Some(index) = lower_text.find("ауд.") {
        let teacher = clean(&normalized[..index]);
        let room = clean(&normalized[index..]);

        return (teacher, room);
    }

    if let Some(index) = lower_text.find("ауд ") {
        let teacher = clean(&normalized[..index]);
        let room = clean(&normalized[index..]);

        return (teacher, room);
    }

    (normalized, String::new())
}

fn find_group_name(document: &Html, group_id: &str) -> String {
    let group_link = Selector::parse("a[id*='hpGroup']").expect("static selector");

    if let Some(link) = document.select(&group_link).next() {
        let name = clean(&link.text().collect::<Vec<_>>().join(" "));

        if !name.is_empty() {
            return name;
        }
    }

    let title = Selector::parse("title").expect("static selector");

    if let Some(node) = document.select(&title).next() {
        let value = clean(&node.text().collect::<Vec<_>>().join(" "));

        if let Some(name) = find_group_label(&value) {
            return name;
        }
    }

    let all_text = clean(&document.root_element().text().collect::<Vec<_>>().join(" "));

    find_group_label(&all_text).unwrap_or_else(|| group_id.to_owned())
}

fn find_group_label(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|part| part.contains('-') && part.chars().any(|c| c.is_ascii_digit()))
        .map(str::to_owned)
}

/// Выбирает таблицу с наибольшим количеством строк расписания.
fn find_schedule_table(document: &Html) -> Option<scraper::ElementRef<'_>> {
    let tables = Selector::parse("table").expect("static selector");
    let data_rows = Selector::parse("tr[id*='_DXDataRow']").expect("static selector");

    let mut best_table = None;
    let mut best_score = 0usize;

    for table in document.select(&tables) {
        let score = table.select(&data_rows).count();

        if score > best_score {
            best_score = score;
            best_table = Some(table);
        }
    }

    if best_table.is_some() {
        return best_table;
    }

    // Запасной вариант для упрощённой HTML-структуры и тестов.
    let rows = Selector::parse("tr").expect("static selector");

    document.select(&tables).find(|table| {
        table
            .select(&rows)
            .any(|row| find_day(&row.text().collect::<Vec<_>>().join(" ")).is_some())
    })
}

/// Разбирает HTML расписания одной группы.
pub fn parse_group_html(html: &str, group_id: &str) -> GroupSchedule {
    let document = Html::parse_document(html);

    let name = find_group_name(&document, group_id);

    let mut lessons: BTreeMap<String, Vec<Lesson>> = BTreeMap::new();

    let Some(table) = find_schedule_table(&document) else {
        return GroupSchedule {
            id: group_id.to_owned(),
            name,
            updated: chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string(),
            lessons,
        };
    };

    let rows = Selector::parse("tr").expect("static selector");
    let cells = Selector::parse("td").expect("static selector");

    let mut day: Option<String> = None;
    let mut last_time: Option<(u8, String, String)> = None;

    for row in table.select(&rows) {
        let row_text = clean(&row.text().collect::<Vec<_>>().join(" "));

        if let Some(day_number) = find_day(&row_text) {
            day = Some(day_number);
            last_time = None;
            continue;
        }

        let Some(day_key) = day.clone() else {
            continue;
        };

        let cell_nodes: Vec<_> = row.select(&cells).collect();

        if cell_nodes.is_empty() {
            continue;
        }

        let cell_values: Vec<String> = cell_nodes
            .iter()
            .map(|cell| cell_lines(*cell).join(" "))
            .collect();

        // В обычной строке время находится в отдельной ячейке.
        // При rowspan в следующей строке ячейки времени уже нет.
        let mut current_time = None;

        for value in &cell_values {
            if let Some(parsed) = parse_time_cell(value) {
                current_time = Some(parsed);
                break;
            }
        }

        if let Some(parsed) = current_time {
            last_time = Some(parsed);
        }

        let Some((pair, start, end)) = last_time.clone() else {
            continue;
        };

        let week = week_from_text(&row_text);

        let mut teacher = String::new();
        let mut room = String::new();
        let mut subject_candidates = Vec::new();

        for value in &cell_values {
            let value_lower = lower(value);

            if value.trim().is_empty() {
                continue;
            }

            if parse_time_cell(value).is_some() {
                continue;
            }

            if value_lower.contains("нечет") || value_lower == "чет" || value_lower == "*" {
                continue;
            }

            if value_lower.contains("ауд.") || value_lower.contains("ауд ") {
                let (found_teacher, found_room) = split_teacher_room(value);

                if !found_teacher.is_empty() {
                    teacher = found_teacher;
                }

                room = found_room;
                continue;
            }

            // Не считаем номер пары или время названием дисциплины.
            if value_lower.contains("пара") {
                continue;
            }

            subject_candidates.push(value.clone());
        }

        // В старом варианте страницы преподаватель и аудитория
        // могли находиться в отдельных ячейках.
        if room.is_empty() && subject_candidates.len() >= 2 {
            let possible_teacher = subject_candidates.iter().position(|value| {
                let value_lower = lower(value);

                value_lower.contains('.') && value.len() < 100
            });

            if let Some(index) = possible_teacher {
                teacher = subject_candidates.remove(index);
            }
        }

        // Название дисциплины обычно находится в самой длинной
        // из оставшихся ячеек.
        let subject = subject_candidates
            .into_iter()
            .max_by_key(|value| value.len())
            .unwrap_or_default();

        let subject_lower = lower(&subject);

        if subject.is_empty()
            || subject_lower.contains("загрузка")
            || subject_lower.contains("ошибка")
        {
            continue;
        }

        lessons.entry(day_key).or_default().push(Lesson {
            pair,
            start,
            end,
            name: subject,
            teacher,
            room,
            week: week.to_owned(),
        });
    }

    GroupSchedule {
        id: group_id.to_owned(),
        name,
        updated: chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string(),
        lessons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_group_links_and_options() {
        let html = r#"
            <a href="/Rasp/Rasp.aspx?group=123&sem=1">ИТ-25-1</a>
            <select>
                <option value="124">Диз-25-1</option>
            </select>
        "#;

        let found = parse_groups_html(html);

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "123");
        assert_eq!(found[1].name, "Диз-25-1");
    }

    #[test]
    fn ignores_semester_options() {
        let html = r#"
            <select>
                <option value="1">Осень</option>
                <option value="2">Весна</option>
                <option value="124">Диз-25-1</option>
            </select>
        "#;

        let found = parse_groups_html(html);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "124");
        assert_eq!(found[0].name, "Диз-25-1");
    }

    #[test]
    fn parses_a_minimal_schedule() {
        let html = r#"
            <title>ИТ-25-1</title>
            <table>
                <tr><td>Понедельник</td></tr>
                <tr>
                    <td>1 пара 09:15<br />10:50</td>
                    <td>Программирование</td>
                    <td>Иванов И.И.</td>
                    <td>ауд. 2.207</td>
                </tr>
            </table>
        "#;

        let parsed = parse_group_html(html, "123");

        assert_eq!(parsed.name, "ИТ-25-1");
        assert_eq!(parsed.lessons.get("1").unwrap()[0].name, "Программирование");
    }

    #[test]
    fn parses_eios_rows_with_rowspan_and_week_markers() {
        let html = r#"
            <title>Расписание</title>
            <a id="ctl00_MainContent_hpGroup">ЛПб-ИСИТ-26-1</a>
            <table>
                <tr id="DXGroupRowExp0"><td>Понедельник</td></tr>
                <tr id="DXDataRow1">
                    <td></td>
                    <td>1 пара 09:15<br />10:50</td>
                    <td>Информатика Лек.</td>
                    <td>Васильев Н.П.<br />ауд. 1.231</td>
                    <td>нечёт</td>
                </tr>
                <tr id="DXDataRow2">
                    <td></td>
                    <td rowspan="2">2 пара 11:10<br />12:45</td>
                    <td>История России Лек.</td>
                    <td>Мельников Н.Н.<br />ауд. 2.409</td>
                    <td>*</td>
                </tr>
                <tr id="DXDataRow3">
                    <td></td>
                    <td>История России Пр.</td>
                    <td>Мельников Н.Н.<br />ауд. 2.409</td>
                    <td>чёт</td>
                </tr>
            </table>
        "#;

        let parsed = parse_group_html(html, "19244");

        assert_eq!(parsed.name, "ЛПб-ИСИТ-26-1");

        let monday = parsed.lessons.get("1").unwrap();

        assert_eq!(monday.len(), 3);
        assert_eq!(monday[0].name, "Информатика Лек.");
        assert_eq!(monday[0].teacher, "Васильев Н.П.");
        assert_eq!(monday[0].room, "ауд. 1.231");
        assert_eq!(monday[0].week, "нечёт");

        assert_eq!(monday[1].start, "11:10");
        assert_eq!(monday[2].start, "11:10");
        assert_eq!(monday[2].week, "чёт");
    }
}
