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
) -> Result<Option<String>, String> {
    let configured: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bot_chat_settings WHERE chat_id = $1 AND is_active = TRUE)"
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

    if let Some(role) = telegram_role {
        if role == "owner" {
            sqlx::query(
                "UPDATE bot_chat_members SET role = 'admin', updated_at = now() WHERE chat_id = $1 AND role = 'owner' AND telegram_id <> $2"
            )
            .bind(chat_id.0)
            .bind(telegram_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }

        sqlx::query(
            r#"
            INSERT INTO bot_chat_members
                (chat_id, telegram_id, role, telegram_status, updated_at)
            VALUES ($1, $2, $3, $4, now())
            ON CONFLICT (chat_id, telegram_id) DO UPDATE SET
                role = CASE
                    WHEN EXCLUDED.role = 'owner' THEN 'owner'
                    WHEN bot_chat_members.role IN ('senior_moderator', 'moderator', 'helper')
                        AND EXCLUDED.role = 'admin' THEN bot_chat_members.role
                    ELSE EXCLUDED.role
                END,
                telegram_status = EXCLUDED.telegram_status,
                updated_at = now()
            "#
        )
        .bind(chat_id.0)
        .bind(telegram_id)
        .bind(role)
        .bind(status)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        if role == "owner" {
            sqlx::query(
                "UPDATE bot_chat_settings SET owner_telegram_id = $2, updated_at = now() WHERE chat_id = $1"
            )
            .bind(chat_id.0)
            .bind(telegram_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
    } else {
        sqlx::query(
            r#"
            INSERT INTO bot_chat_members
                (chat_id, telegram_id, role, telegram_status, updated_at)
            VALUES ($1, $2, 'member', $3, now())
            ON CONFLICT (chat_id, telegram_id) DO UPDATE SET
                role = CASE
                    WHEN bot_chat_members.role IN ('owner', 'admin') THEN 'member'
                    ELSE bot_chat_members.role
                END,
                telegram_status = EXCLUDED.telegram_status,
                updated_at = now()
            "#
        )
        .bind(chat_id.0)
        .bind(telegram_id)
        .bind(status)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    let role: String = sqlx::query_scalar(
        "SELECT role FROM bot_chat_members WHERE chat_id = $1 AND telegram_id = $2"
    )
    .bind(chat_id.0)
    .bind(telegram_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(Some(role))
}

/// Telegram administrators and the chat creator always have administrative access.
pub async fn can_manage_chat(
    bot: &Bot,
    db: &PgPool,
    chat_id: ChatId,
    telegram_id: i64,
) -> bool {
    match sync_member(bot, db, chat_id, telegram_id).await {
        Ok(Some(role)) => matches!(
            role.as_str(),
            "owner" | "admin" | "senior_moderator" | "moderator"
        ),
        Ok(None) | Err(_) => false,
    }
}
