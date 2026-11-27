use std::{env, io::Write, process::ExitCode, time::Duration};

use smart_campus_backend::services::{eios_sync::EiosSync, schedule_sync};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Ошибка синхронизации: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL")?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&database_url)
        .await?;

    // Keep the CLI in sync with the backend schema before importing schedule data.
    sqlx::migrate!("./migrations").run(&pool).await?;
    let eios = EiosSync::new("data/eios_cache", "1")?;

    println!("Загружаю актуальное расписание ЕИОС в локальную PostgreSQL БД…");
    let report = schedule_sync::sync_all_groups_with_progress(&pool, &eios, |done, total| {
        println!("SYNC_PROGRESS={done}/{total}");
        let _ = std::io::stdout().flush();
    })
    .await?;
    println!(
        "Готово: успешно {}/{} групп, ошибок {}, импортировано занятий {}.",
        report.groups_successful,
        report.groups_total,
        report.groups_failed,
        report.lessons_imported
    );
    if report.groups_total > 0 && report.groups_successful == 0 {
        return Ok(ExitCode::FAILURE);
    }
    if report.groups_failed > 0 {
        println!("Часть групп не обновилась; прежние данные для них сохранены.");
    }
    Ok(ExitCode::SUCCESS)
}
