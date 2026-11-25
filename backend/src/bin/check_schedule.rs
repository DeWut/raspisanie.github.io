use smart_campus_backend::services::parser::parse_group_html;

fn main() {
    let html =
        std::fs::read_to_string("schedule.html").expect("Не удалось прочитать schedule.html");

    let schedule = parse_group_html(&html, "19244");

    println!("Группа: {}", schedule.name);

    let mut total = 0;

    for (day, lessons) in &schedule.lessons {
        println!("\nДень недели: {}", day);

        for lesson in lessons {
            println!(
                "Пара {} | {}-{} | {} | {} | {} | {}",
                lesson.pair,
                lesson.start,
                lesson.end,
                lesson.name,
                lesson.teacher,
                lesson.room,
                lesson.week
            );
        }

        println!("Занятий за день: {}", lessons.len());
        total += lessons.len();
    }

    println!("\nВсего найдено занятий: {}", total);
}
