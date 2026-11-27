use std::{env, io::Write, process::ExitCode, time::Duration};

use smart_campus_backend::services::{eios_sync::EiosSync, schedule_sync};
use sqlx::postgres::PgPoolOptions;
use url::Url;

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


fn direct_neon_url(database_url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut parsed = Url::parse(database_url)?;
    if let Some(host) = parsed.host_str() {
        if host.contains("-pooler.") {
            parsed.set_host(Some(&host.replace("-pooler.", ".")))?;
        }
    }
    Ok(parsed.to_string())
}

async fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL")?;
    let migration_url = match env::var("DATABASE_URL_UNPOOLED") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => direct_neon_url(&database_url)?,
    };

    // Neon migrations must use the direct endpoint: SQLx migration locking
    // relies on session state and is not compatible with PgBouncer transaction pooling.
    let migration_pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .connect(&migration_url)
        .await?;
    sqlx::migrate!("./migrations").run(&migration_pool).await?;
    migration_pool.close().await;

    // Normal schedule queries can use Neon's pooled endpoint.
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(30))
        .connect(&database_url)
        .await?;
    let eios = EiosSync::new("data/eios_cache", "1")?;

    println!("Загружаю актуальное расписание ЕИОС в подключённую PostgreSQL БД…");
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
