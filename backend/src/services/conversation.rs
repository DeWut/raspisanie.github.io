//! Deterministic Russian-language intent routing. No generated answers or remote model.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    Help,
    Greeting,
    Today,
    Tomorrow,
    Week,
    NextLesson,
    Homework,
    Deadlines,
    FreeRooms(Option<String>),
    Teacher(String),
}

fn normalize(text: &str) -> String {
    text.to_lowercase()
        .replace('ё', "е")
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ':' || c == '-' { c } else { ' ' })
        .collect::<String>()
}

fn has_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| text.contains(word))
}

fn time_in(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        let value = token.strip_prefix("на").unwrap_or(token);
        if let Some((hour, minute)) = value.split_once(':') {
            if hour.len() <= 2 && minute.len() == 2
                && hour.parse::<u8>().is_ok_and(|v| v < 24)
                && minute.parse::<u8>().is_ok_and(|v| v < 60)
            {
                return Some(format!("{hour:0>2}:{minute}"));
            }
        }
    }
    None
}

pub fn detect(text: &str) -> Option<Intent> {
    let text = normalize(text);
    if text.trim().is_empty() { return None; }

    if has_any(&text, &["аудитор", "ауд.", "свободн", "корпус"])
        && has_any(&text, &["аудитор", "ауд.", "свободн"])
    {
        return Some(Intent::FreeRooms(time_in(&text)));
    }
    if has_any(&text, &["дедлайн", "срок сдач", "когда сда", "что сдать", "надо сдать"])
        || (text.contains("сдать") && has_any(&text, &["когда", "что", "срок"]))
    {
        return Some(Intent::Deadlines);
    }
    if has_any(&text, &["домашн", "дз", "задали", "задание", "задания", "домашка"])
        && has_any(&text, &["какое", "какие", "покажи", "есть", "задан", "дз", "домашн"])
    {
        return Some(Intent::Homework);
    }
    if has_any(&text, &["завтра", "на завтра"]) { return Some(Intent::Tomorrow); }
    if has_any(&text, &["недел", "7 дней", "семь дней"]) { return Some(Intent::Week); }
    if has_any(&text, &["следующ", "ближайш", "после этого", "дальше"])
        && has_any(&text, &["пар", "занят", "урок", "что"])
    { return Some(Intent::NextLesson); }
    if has_any(&text, &["сегодня", "сейчас", "расписан", "пары", "пара", "занятия"])
    { return Some(Intent::Today); }

    if has_any(&text, &["преподавател", "препод", "кто ведет", "кто ведет", "когда у"])
        && text.split_whitespace().count() >= 2
    {
        let query = text.split_whitespace()
            .filter(|word| !["найди", "расписание", "преподаватель", "преподавателя", "препод", "когда", "у", "занятия", "ведет", "ведёт"].contains(word))
            .collect::<Vec<_>>().join(" ");
        if query.chars().count() >= 3 { return Some(Intent::Teacher(query)); }
    }
    if has_any(&text, &["привет", "здравствуй", "здаров", "барни", "барней", "спбфту"]) {
        return Some(Intent::Greeting);
    }
    if has_any(&text, &["помощь", "что умеешь", "что можешь", "как пользоваться", "команды", "меню"])
    { return Some(Intent::Help); }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_schedule_homework_rooms_and_teacher_without_ai() {
        assert_eq!(detect("какое расписание завтра?"), Some(Intent::Tomorrow));
        assert_eq!(detect("какое дз задали?"), Some(Intent::Homework));
        assert_eq!(detect("свободные аудитории на 20:00"), Some(Intent::FreeRooms(Some("20:00".into()))));
        assert_eq!(detect("найди преподавателя Иванов"), Some(Intent::Teacher("иванов".into())));
    }
}
