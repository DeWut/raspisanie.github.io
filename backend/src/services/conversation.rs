//! Deterministic Russian-language intent routing. No generated answers or remote model.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    Help,
    Greeting,
    Today,
    Tomorrow,
    Week,
    Weekday(String),
    NextLesson,
    Homework,
    HomeworkFor(String),
    Deadlines,
    FreeRooms(Option<String>),
    Teacher(String),
    Profile,
    Settings,
    GroupChangeInfo,
    Features,
    GroupLookup(String),
    ScheduleChanges,
    HomeworkManagement,
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

fn intent_tokens(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "а","и","но","или","мне","меня","моя","мой","мои","у","в","во","на","по","за","для",
        "с","со","из","к","ко","от","до","же","ли","бы","это","этот","эта","эти","как",
        "что","какой","какая","какие","какое","когда","где","почему","можно","нужно","надо",
        "пожалуйста","мне","ты","вы","у","нас","унас","есть","будет","будут","сейчас"
    ];
    text.split_whitespace()
        .filter(|token| !STOP.contains(token) && token.chars().count() > 1)
        .map(|token| {
            let mut word = token.to_owned();
            for suffix in [
                "иями","ями","ами","ями","ого","его","ому","ему","ыми","ими","ую","юю",
                "ая","яя","ое","ее","ые","ие","ый","ий","ой","ей","ам","ям","ах","ях",
                "ов","ев","ом","ем","а","я","ы","и","е","о","у","ю"
            ] {
                if word.ends_with(suffix) && word.chars().count() > suffix.chars().count()+2 {
                    word.truncate(word.len()-suffix.len());
                    break;
                }
            }
            word
        })
        .collect()
}

fn edit_distance_at_most_one(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 1 { return false; }
    let (mut i, mut j, mut edits) = (0usize, 0usize, 0u8);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] { i += 1; j += 1; continue; }
        edits += 1;
        if edits > 1 { return false; }
        if a.len() > b.len() { i += 1; }
        else if b.len() > a.len() { j += 1; }
        else { i += 1; j += 1; }
    }
    if i < a.len() || j < b.len() { edits += 1; }
    edits <= 1
}

fn phrase_similarity(message: &[String], example: &[String]) -> f32 {
    if message.is_empty() || example.is_empty() { return 0.0; }
    let mut matched = 0usize;
    for token in message {
        if example.iter().any(|candidate| {
            token == candidate || (token.chars().count() >= 5
                && candidate.chars().count() >= 5
                && edit_distance_at_most_one(token, candidate))
        }) {
            matched += 1;
        }
    }
    (2 * matched) as f32 / (message.len() + example.len()) as f32
}

fn dictionary_intent(text: &str) -> Option<Intent> {
    use std::sync::OnceLock;
    static EXAMPLES: OnceLock<Vec<(String, Vec<Vec<String>>)>> = OnceLock::new();
    let dictionary = EXAMPLES.get_or_init(|| {
        let parsed: serde_json::Value = serde_json::from_str(include_str!("../../data/barney_ru_intents.json"))
            .expect("Barney Russian intent dictionary must be valid JSON");
        parsed.as_array().into_iter().flatten().filter_map(|entry| {
            let name = entry.get("intent")?.as_str()?.to_owned();
            let examples = entry.get("examples")?.as_array()?.iter()
                .filter_map(|v| v.as_str().map(intent_tokens))
                .filter(|v| !v.is_empty()).collect::<Vec<_>>();
            Some((name, examples))
        }).collect()
    });
    let tokens = intent_tokens(text);
    if tokens.is_empty() { return None; }
    let mut scores = dictionary.iter().map(|(name, examples)| {
        let best = examples.iter().map(|example| phrase_similarity(&tokens, example))
            .fold(0.0_f32, f32::max);
        (name.as_str(), best)
    }).collect::<Vec<_>>();
    scores.sort_by(|a,b| b.1.total_cmp(&a.1));
    let (name, score) = *scores.first()?;
    let second = scores.get(1).map(|v| v.1).unwrap_or(0.0);
    // Conservative fallback: uncertain text is clarified instead of routed to
    // a potentially destructive or unrelated command.
    if score < 0.50 || score - second < 0.07 { return None; }
    match name {
        "today" => Some(Intent::Today),
        "tomorrow" => Some(Intent::Tomorrow),
        "week" => Some(Intent::Week),
        "next_lesson" => Some(Intent::NextLesson),
        "homework" => Some(Intent::Homework),
        "deadlines" => Some(Intent::Deadlines),
        "free_rooms" => Some(Intent::FreeRooms(time_in(text))),
        "teacher" => {
            let query = text.split_whitespace().filter(|word| ![
                "найди","расписание","преподаватель","преподавателя","преподаватели","препод",
                "когда","какой","какая","какие","у","занятия","занятие","ведет","ведёт",
                "читает","кто","пары","пара","будут","есть","сегодня","завтра","во","сколько",
                "покажи","найти","учителя","доцента","профессора"
            ].contains(word)).collect::<Vec<_>>().join(" ");
            (query.chars().count() >= 3).then_some(Intent::Teacher(query))
        },
        "profile" => Some(Intent::Profile),
        "settings" => Some(Intent::Settings),
        "group_change" => Some(Intent::GroupChangeInfo),
        "features" => Some(Intent::Features),
        "schedule_changes" => Some(Intent::ScheduleChanges),
        "homework_management" => Some(Intent::HomeworkManagement),
        "help" => Some(Intent::Help),
        "greeting" => Some(Intent::Greeting),
        _ => None,
    }
}

fn weekday_date(text: &str) -> Option<String> {
    use chrono::{Datelike, Duration, FixedOffset, Utc};
    let lower = normalize(text);
    let target = [
        ("понедельник", 0i64), ("вторник", 1), ("среду", 2), ("среда", 2),
        ("четверг", 3), ("пятницу", 4), ("пятница", 4), ("субботу", 5),
        ("суббота", 5), ("воскресенье", 6),
    ].iter().find(|(word, _)| lower.contains(word))?.1;
    let moscow = FixedOffset::east_opt(3 * 60 * 60)?;
    let today = Utc::now().with_timezone(&moscow).date_naive();
    let current = i64::from(today.weekday().num_days_from_monday());
    let offset = (target - current + 7) % 7;
    Some((today + Duration::days(offset)).format("%Y-%m-%d").to_string())
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

/// Extracts a short subject key from phrases such as "ДЗ по математике"
/// and conversational follow-ups such as "а по математике?". The key is
/// deliberately stemmed so common Russian case endings still match DB names.
pub fn homework_subject(text: &str) -> Option<String> {
    let normalized = normalize(text);
    let after = normalized.split_once("по предмету ")
        .map(|(_, value)| value)
        .or_else(|| normalized.split_once("по ").map(|(_, value)| value))?;
    let token = after.split_whitespace()
        .find(|word| !["дз", "домашка", "домашнее", "задание", "задания", "задали", "предмету", "а", "и", "что", "мне", "нам"].contains(word))?;
    let mut subject = token.to_owned();
    for suffix in ["иями", "ями", "ами", "ого", "ему", "ому", "иях", "иях", "ии"] {
        if subject.ends_with(suffix) && subject.len() > suffix.len() + 2 {
            subject.truncate(subject.len() - suffix.len());
            break;
        }
    }
    if subject.ends_with('е') && subject.len() > 4 {
        subject.pop();
    } else if (subject.ends_with("ку") || subject.ends_with("гу")) && subject.len() > 4 {
        subject.pop();
    } else if (subject.ends_with("ой") || subject.ends_with("ей")) && subject.len() > 4 {
        subject.truncate(subject.len() - 2);
    }
    (subject.chars().count() >= 4).then_some(subject)
}

/// Resolves short follow-ups using the previous user turn, without retaining
/// unbounded personal conversation history or guessing when the topic is unclear.
pub fn detect_with_context(
    text: &str,
    previous_user: Option<&str>,
    previous_assistant: Option<&str>,
) -> Option<Intent> {
    let lower = normalize(text);
    let is_followup = lower.starts_with("а ") || lower.starts_with("и ")
        || lower.starts_with("а если") || lower.starts_with("а когда")
        || lower.starts_with("а где") || lower.starts_with("а сколько")
        || lower.starts_with("а по ") || lower.starts_with("и по ")
        || lower.starts_with("почему ") || lower.starts_with("тогда ")
        || lower.starts_with("а это ") || lower.starts_with("а там ");
    let prior = previous_user.and_then(detect);
    // Resolve a contextual reference before fuzzy matching: "а где аудитория?"
    // after asking about the next lesson is still about that lesson, not a room search.
    if is_followup && matches!(prior, Some(Intent::NextLesson))
        && has_any(&lower, &["аудитор", "время", "преподавател", "кабинет"]) {
        return Some(Intent::NextLesson);
    }
    if let Some(intent) = detect(text) { return Some(intent); }
    if !is_followup { return None; }
    match prior {
        Some(Intent::Today | Intent::Tomorrow | Intent::Week | Intent::NextLesson) => {
            if has_any(&lower, &["завтра", "на следующий день"]) { Some(Intent::Tomorrow) }
            else if has_any(&lower, &["сегодня", "на текущий день"]) { Some(Intent::Today) }
            else if has_any(&lower, &["недел", "выходные", "всю неделю"]) { Some(Intent::Week) }
            else if has_any(&lower, &["а аудитория", "в какой аудитории", "кто ведет", "кто ведет"]) { Some(Intent::NextLesson) }
            else { Some(Intent::Week) }
        }
        Some(Intent::Homework | Intent::HomeworkFor(_)) => {
            homework_subject(text).map(Intent::HomeworkFor).or(Some(Intent::Homework))
        }
        Some(Intent::Deadlines) => Some(Intent::Deadlines),
        Some(Intent::FreeRooms(_)) => Some(Intent::FreeRooms(time_in(text))),
        Some(Intent::Teacher(_)) => detect(text).or_else(|| {
            let query = text.split_whitespace().filter(|w| !["а","и","когда","где","какие","пары","занятия","у"].contains(w)).collect::<Vec<_>>().join(" ");
            (query.chars().count() >= 3).then_some(Intent::Teacher(query))
        }),
        Some(Intent::Profile) => Some(Intent::Profile),
        Some(Intent::Settings | Intent::GroupChangeInfo) => Some(Intent::Settings),
        Some(Intent::ScheduleChanges) => Some(Intent::ScheduleChanges),
        _ => {
            let topic = previous_assistant.unwrap_or("").to_lowercase();
            if topic.contains("расписан") || topic.contains("следующая пара") {
                if has_any(&lower, &["завтра"]) { Some(Intent::Tomorrow) } else { Some(Intent::NextLesson) }
            } else if topic.contains("домаш") || topic.contains("задан") || topic.contains("дз") {
                homework_subject(text).map(Intent::HomeworkFor).or(Some(Intent::Homework))
            } else if topic.contains("дедлайн") || topic.contains("срок") {
                Some(Intent::Deadlines)
            } else { None }
        }
    }
}

pub fn detect(text: &str) -> Option<Intent> {
    let text = normalize(text)
        .replace("дщ", "дз")
        .replace("дс", "дз")
        .replace("расписане", "расписание")
        .replace("расписанье", "расписание");
    if text.trim().is_empty() { return None; }

    if has_any(&text, &["расписание", "расписан"]) && has_any(&text, &["группа", "группы", "группу"]) {
        if let Some(candidate) = text.split_whitespace().find(|word| word.contains('-') && word.chars().any(|c| c.is_ascii_digit())) {
            return Some(Intent::GroupLookup(candidate.to_owned()));
        }
    }
    if has_any(&text, &["какая у меня группа", "какая моя группа", "моя группа", "что ты обо мне знаешь", "ты обо мне знаешь", "какие у меня персональные настройки", "мои персональные данные", "мой профиль", "мой аккаунт"]) {
        return Some(Intent::Profile);
    }
    if (has_any(&text, &["измен", "смен", "поменя"]) && has_any(&text, &["групп"]) && has_any(&text, &["сохран", "настройк"])) ||
       has_any(&text, &["если я изменю группу", "при смене группы", "сменю группу", "поменяю группу", "изменю учебную группу", "настройки останутся", "настройки сохранятся"]) {
        return Some(Intent::GroupChangeInfo);
    }
    if has_any(&text, &["настройк", "параметр аккаунта", "уведомления включ", "личный кабинет", "центр управления", "что можно изменить"]) {
        return Some(Intent::Settings);
    }
    if has_any(&text, &["система домашних заданий", "как работает дз", "как работает домашнее задание", "добавлять задания", "редактировать задания", "удалять задания", "управлять дз", "управление дз"]) {
        return Some(Intent::HomeworkManagement);
    }
    if has_any(&text, &["знакомств", "возможности mini app", "функции mini app", "какие функции", "что умеешь", "что можешь", "список возможностей"]) {
        return Some(Intent::Features);
    }
    if has_any(&text, &["список команд", "какие команды", "команды бота", "команду", "команды", "помощь", "как пользоваться", "меню", "help"]) {
        return Some(Intent::Help);
    }

    if has_any(&text, &["следующая пара", "следующую пару", "ближайшая пара", "ближайшую пару", "что за пара дальше", "какая пара дальше", "после какой пары"]) {
        return Some(Intent::NextLesson);
    }
    if has_any(&text, &["свободн", "пустые аудитории", "свободные кабинеты"]) {
        return Some(Intent::FreeRooms(time_in(&text)));
    }
    if has_any(&text, &["преподавател", "препод", "кто ведет", "кто ведёт", "кто читает", "занятия у", "пары у", "когда у"]) {
        let query = text.split_whitespace().filter(|word| ![
            "найди","расписание","преподаватель","преподавателя","преподаватели","препод",
            "когда","какой","какая","какие","у","занятия","занятие","ведет","ведёт",
            "читает","кто","пары","пара","будут","есть","сегодня","завтра","во","сколько",
            "покажи","найти"
        ].contains(word)).collect::<Vec<_>>().join(" ");
        if query.chars().count() >= 3 { return Some(Intent::Teacher(query)); }
    }
    if has_any(&text, &["дедлайн", "срок сдач", "когда сда", "что сдать", "надо сдать", "сроки", "к какому числу", "до какого числа"]) {
        return Some(Intent::Deadlines);
    }
    if has_any(&text, &["домашн", "домашк", "дз", "задали", "задание", "задания", "список дз", "список заданий", "что задали", "что задано", "домашка"]) {
        return Some(homework_subject(&text).map(Intent::HomeworkFor).unwrap_or(Intent::Homework));
    }
    if has_any(&text, &["изменения в расписании", "изменилось расписание", "изменения или отмены", "отменили пары", "перенесли пары", "замены", "переносы занятий"]) {
        return Some(Intent::ScheduleChanges);
    }
    if has_any(&text, &["экзамен", "зачет", "зачёт", "сессия"]) {
        return Some(Intent::Help);
    }
    if has_any(&text, &["завтра", "на завтра"]) { return Some(Intent::Tomorrow); }
    if has_any(&text, &["понедельник", "вторник", "среду", "среда", "четверг", "пятницу", "пятница", "субботу", "суббота", "воскресенье"]) {
        if let Some(date) = weekday_date(&text) { return Some(Intent::Weekday(date)); }
    }
    if has_any(&text, &["недел", "7 дней", "семь дней", "ближайшие дни"]) {
        return Some(Intent::Week);
    }
    if has_any(&text, &["сегодня", "сейчас", "расписан", "пары", "пара", "занятия", "уроки"]) {
        return Some(Intent::Today);
    }
    if has_any(&text, &["привет", "здравствуй", "здаров", "доброе утро", "добрый день", "добрый вечер", "барни", "барней", "спбфту"]) {
        return Some(Intent::Greeting);
    }
    dictionary_intent(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_handles_typos_and_natural_word_order() {
        assert_eq!(dictionary_intent("покажи расписание на завтр"), Some(Intent::Tomorrow));
        assert_eq!(dictionary_intent("напиши список команд бота"), Some(Intent::Help));
        assert_eq!(dictionary_intent("можешь показать свободные аудитории"), Some(Intent::FreeRooms(None)));
    }

    #[test]
    fn contextual_followups_keep_the_previous_topic() {
        assert_eq!(
            detect_with_context("а где следующая аудитория?", Some("Какая у меня следующая пара?"), None),
            Some(Intent::NextLesson)
        );
        assert_eq!(
            detect_with_context("а по физике?", Some("Покажи список ДЗ"), None),
            Some(Intent::HomeworkFor("физик".into()))
        );
        assert_eq!(
            detect_with_context("а завтра?", Some("Покажи расписание на сегодня"), None),
            Some(Intent::Tomorrow)
        );
        assert_eq!(
            detect_with_context("а какие сроки?", Some("Покажи список домашних заданий"), None),
            Some(Intent::Deadlines)
        );
    }

    #[test]
    fn detects_schedule_homework_rooms_and_teacher_without_ai() {
        assert_eq!(detect("какое расписание завтра?"), Some(Intent::Tomorrow));
        assert!(matches!(detect("покажи расписание на понедельник"), Some(Intent::Weekday(_))));
        assert_eq!(detect("какое дз задали?"), Some(Intent::Homework));
        assert_eq!(detect("свободные аудитории на 20:00"), Some(Intent::FreeRooms(Some("20:00".into()))));
        assert_eq!(detect("найди преподавателя Иванов"), Some(Intent::Teacher("иванов".into())));
        assert_eq!(detect("когда у Иванова пары?"), Some(Intent::Teacher("иванова".into())));
        assert_eq!(detect("что у нас завтра?"), Some(Intent::Tomorrow));
        assert_eq!(detect("Барни, привет!"), Some(Intent::Greeting));
        assert_eq!(detect("Привет, какое ДЗ?"), Some(Intent::Homework));
        assert_eq!(detect("А какое домашнее задание задали?"), Some(Intent::Homework));
        assert_eq!(detect("Что задали по улв?"), Some(Intent::Homework));
        assert_eq!(detect("Покажи домашку"), Some(Intent::Homework));
        assert_eq!(detect("Что задали по математике?"), Some(Intent::HomeworkFor("математик".into())));
        assert_eq!(homework_subject("а по физике?"), Some("физик".into()));
        assert_eq!(homework_subject("ДЗ по математике"), Some("математик".into()));
        assert_eq!(homework_subject("задали по истории"), Some("истор".into()));
        assert_eq!(homework_subject("а по английскому?"), Some("английск".into()));
        assert_eq!(detect("Какие занятия у Иванова?"), Some(Intent::Teacher("иванова".into())));
        assert_eq!(detect("Во сколько у нас пара?"), Some(Intent::Today));
        assert_eq!(detect("Добрый вечер!"), Some(Intent::Greeting));
        assert_eq!(detect("Когда сдавать домашку?"), Some(Intent::Deadlines));
        assert_eq!(detect("Барни, напиши список команд"), Some(Intent::Help));
        assert_eq!(detect("Какая у меня следующая пара? Укажи аудиторию, преподавателя и время."), Some(Intent::NextLesson));
        assert_eq!(detect("Покажи список дщ"), Some(Intent::Homework));
        assert_eq!(detect("Какая у меня сейчас установлена группа? Какие персональные настройки ты обо мне знаешь?"), Some(Intent::Profile));
        assert_eq!(detect("Если я изменю свою группу, сохранятся ли мои остальные настройки?"), Some(Intent::GroupChangeInfo));
        assert_eq!(detect("Покажи расписание несуществующей группы TEST-999."), Some(Intent::GroupLookup("test-999".into())));
        assert_eq!(detect("Есть ли изменения или отмены в расписании?"), Some(Intent::ScheduleChanges));
        assert_eq!(detect("Какие настройки я могу изменить самостоятельно?"), Some(Intent::Settings));
        assert_eq!(detect("Как работает система знакомств и личных кабинетов?"), Some(Intent::Features));
    }
}
