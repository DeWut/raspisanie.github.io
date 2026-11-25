use sqlx::PgPool;
use teloxide::{prelude::*, types::UserId};

/// Refreshes a member's Telegram status from Telegram itself.
/// SMART CAMPUS roles are preserved for ordinary members; Telegram owner/admin
/// status is always treated as a stronger permission source.
pub async fn sync_member(
    bot: &Bot,
    db: &PgPool,
    chat_id: ChatId,
    telegram_id: i64,
    expected_username: Option<&str>,
) -> Result<Option<String>, String> {
    let configured: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id = $1 AND is_active = TRUE)",
    )
    .bind(chat_id.0)
    .fetch_one(db)
    .await
    .map_err(|e| e.to_string())?;

    if !configured {
        return Ok(None);
    }

    let user_id = u64::try_from(telegram_id).map_err(|e| e.to_string())?;
    let member = bot
        .get_chat_member(chat_id, UserId(user_id))
        .await
        .map_err(|e| e.to_string())?;

    let actual_username = member.user.username.as_deref();
    if let Some(expected_username) = expected_username {
        let expected = expected_username.trim_start_matches('@');
        if !actual_username.is_some_and(|actual| actual.eq_ignore_ascii_case(expected)) {
            return Err(format!(
                "Telegram username @{} no longer matches the cached chat member. Ask the user to send a message or assign the role by replying to their message.",
                expected
            ));
        }
    }

    remember_member_identity(db, chat_id, telegram_id, actual_username).await?;

    let (status, telegram_role) = if member.kind.is_owner() {
        ("creator", Some("owner"))
    } else if member.kind.is_administrator() {
        ("administrator", Some("admin"))
    } else if member.kind.is_restricted() {
        ("restricted", None)
    } else if member.kind.is_left() {
        ("left", None)
    } else if member.kind.is_banned() {
        ("kicked", None)
    } else {
        ("member", None)
    };

    let mut tx = db.begin().await.map_err(|e| e.to_string())?;

    // The role column stores only SMART CAMPUS roles. Telegram status is
    // stored separately and is the source of truth for owner/admin privileges.
    sqlx::query(
        r#"
        INSERT INTO bot_chat_members
            (chat_id, telegram_id, role, telegram_status, updated_at)
        VALUES ($1, $2, 'member', $3, now())
        ON CONFLICT (chat_id, telegram_id) DO UPDATE SET
            telegram_status = EXCLUDED.telegram_status,
            updated_at = now()
        "#,
    )
    .bind(chat_id.0)
    .bind(telegram_id)
    .bind(status)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    if telegram_role == Some("owner") {
        sqlx::query(
            "UPDATE bot_chat_settings SET owner_telegram_id = $2, updated_at = now() WHERE chat_id = $1"
        )
        .bind(chat_id.0)
        .bind(telegram_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    let smart_role: String = sqlx::query_scalar(
        "SELECT role FROM bot_chat_members WHERE chat_id = $1 AND telegram_id = $2",
    )
    .bind(chat_id.0)
    .bind(telegram_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;

    // Do not grant bot permissions to users who are no longer active members.
    if matches!(status, "left" | "kicked" | "restricted") {
        return Ok(None);
    }

    Ok(Some(match telegram_role {
        Some("owner") => "owner".to_owned(),
        Some("admin") => "admin".to_owned(),
        _ => smart_role,
    }))
}

/// Stores usernames for members the bot has actually seen in this configured chat.
/// This cache enables /role @username while avoiding guesses based on public profile names.
pub async fn remember_member_identity(
    db: &PgPool,
    chat_id: ChatId,
    telegram_id: i64,
    username: Option<&str>,
) -> Result<(), String> {
    let mut tx = db.begin().await.map_err(|e| e.to_string())?;

    if let Some(username) = username {
        // A Telegram username can be reassigned. Clear any older cached owner
        // of this username before recording the person who currently uses it.
        sqlx::query(
            r#"
            UPDATE bot_chat_members
            SET username = NULL, updated_at = now()
            WHERE chat_id = $1
              AND lower(username) = lower($2)
              AND telegram_id <> $3
            "#,
        )
        .bind(chat_id.0)
        .bind(username)
        .bind(telegram_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    sqlx::query(
        r#"
        INSERT INTO bot_chat_members
            (chat_id, telegram_id, role, telegram_status, username)
        SELECT $1, $2, 'member', 'member', $3
        WHERE EXISTS (
            SELECT 1 FROM bot_chat_settings
            WHERE chat_id = $1 AND is_active = TRUE
        )
        ON CONFLICT (chat_id, telegram_id) DO UPDATE SET
            username = EXCLUDED.username,
            updated_at = now()
        "#,
    )
    .bind(chat_id.0)
    .bind(telegram_id)
    .bind(username)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())
}

/// Resolves a username only from identities observed in this chat.
pub async fn find_member_by_username(
    db: &PgPool,
    chat_id: ChatId,
    username: &str,
) -> Result<Option<i64>, String> {
    sqlx::query_scalar(
        r#"
        SELECT telegram_id
        FROM bot_chat_members
        WHERE chat_id = $1 AND lower(username) = lower($2)
        "#,
    )
    .bind(chat_id.0)
    .bind(username.trim_start_matches('@'))
    .fetch_optional(db)
    .await
    .map_err(|e| e.to_string())
}

/// Telegram administrators and the chat creator always have administrative access.
pub async fn can_manage_chat(bot: &Bot, db: &PgPool, chat_id: ChatId, telegram_id: i64) -> bool {
    match sync_member(bot, db, chat_id, telegram_id, None).await {
        Ok(Some(role)) => matches!(
            role.as_str(),
            "owner" | "admin" | "senior_moderator" | "moderator"
        ),
        Ok(None) | Err(_) => false,
    }
}
