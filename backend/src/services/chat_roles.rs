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
            "UPDATE bot_chat_settings SET owner_telegram_id = COALESCE(owner_telegram_id, $2), updated_at = now() WHERE chat_id = $1"
        )
        .bind(chat_id.0)
        .bind(telegram_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    let (smart_role, is_smart_owner): (String, bool) = sqlx::query_as(
        r#"
        SELECT m.role, COALESCE(s.owner_telegram_id = m.telegram_id, FALSE)
        FROM bot_chat_members m
        LEFT JOIN bot_chat_settings s
          ON s.chat_id = m.chat_id AND s.is_active = TRUE
        WHERE m.chat_id = $1 AND m.telegram_id = $2
        "#,
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
        _ if is_smart_owner => "owner".to_owned(),
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

/// Checks a moderation capability against the chat's role hierarchy.
/// Telegram creator/admin always pass; helper can warn, moderator can mute,
/// senior moderator can ban. Assignment of roles remains owner/admin-only.
async fn has_moderation_level(
    bot: &Bot,
    db: &PgPool,
    chat_id: ChatId,
    telegram_id: i64,
    minimum_level: u8,
) -> bool {
    let level = match sync_member(bot, db, chat_id, telegram_id, None).await {
        Ok(Some(role)) => match role.as_str() {
            "owner" | "admin" => 3,
            "senior_moderator" => 3,
            "moderator" => 2,
            "helper" => 1,
            _ => 0,
        },
        Ok(None) | Err(_) => 0,
    };
    level >= minimum_level
}

/// Permission to issue and review warnings.
pub async fn can_warn_chat(bot: &Bot, db: &PgPool, chat_id: ChatId, telegram_id: i64) -> bool {
    has_moderation_level(bot, db, chat_id, telegram_id, 1).await
}

/// Permission to mute and unmute members.
pub async fn can_mute_chat(bot: &Bot, db: &PgPool, chat_id: ChatId, telegram_id: i64) -> bool {
    has_moderation_level(bot, db, chat_id, telegram_id, 2).await
}

/// Permission to ban and unban members.
pub async fn can_ban_chat(bot: &Bot, db: &PgPool, chat_id: ChatId, telegram_id: i64) -> bool {
    has_moderation_level(bot, db, chat_id, telegram_id, 3).await
}

/// Compatibility alias for general moderation actions.
pub async fn can_manage_chat(bot: &Bot, db: &PgPool, chat_id: ChatId, telegram_id: i64) -> bool {
    can_mute_chat(bot, db, chat_id, telegram_id).await
}

/// Transfers SMART CAMPUS ownership to a member already observed in this chat.
/// Telegram's own creator/admin status remains authoritative and is never changed.
pub async fn handle_ownership(
    bot: &Bot,
    db: &PgPool,
    chat_id: ChatId,
    actor: i64,
    command: &str,
    argument: &str,
) -> Option<String> {
    if !matches!(command, "/transfer_owner" | "/owner") || chat_id.0 >= 0 {
        return None;
    }
    let current_owner: Option<i64> = sqlx::query_scalar(
        "SELECT owner_telegram_id FROM bot_chat_settings WHERE chat_id=$1 AND is_active=TRUE"
    ).bind(chat_id.0).fetch_optional(db).await.ok().flatten();
    let telegram_creator: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bot_chat_members WHERE chat_id=$1 AND telegram_id=$2 AND telegram_status='creator')"
    ).bind(chat_id.0).bind(actor).fetch_one(db).await.unwrap_or(false);
    if current_owner != Some(actor) && !telegram_creator {
        return Some("⛔ Передать владение может только владелец SMART CAMPUS или создатель Telegram-чата.".into());
    }
    if command == "/owner" || argument.trim().is_empty() {
        return Some(format!("👑 Владелец SMART CAMPUS: {}\nПередача: /transfer_owner @username",current_owner.map(|v|v.to_string()).unwrap_or_else(||"не назначен".into())));
    }
    let target = argument.trim().trim_start_matches('@');
    let Some(target_id) = (match find_member_by_username(db, chat_id, target).await {
        Ok(value) => value,
        Err(_) => None,
    }) else {
        return Some("Не нашёл участника по этому @username. Попроси его написать сообщение в группе и повтори команду.".into());
    };
    if target_id == actor {
        return Some("Ты уже владелец SMART CAMPUS.".into());
    }
    let target_user_id = match u64::try_from(target_id) {
        Ok(id) if id > 0 => id,
        _ => return Some("Некорректный Telegram ID участника.".into()),
    };
    let active_member = match bot.get_chat_member(chat_id, UserId(target_user_id)).await {
        Ok(member) => !member.kind.is_left() && !member.kind.is_banned(),
        Err(_) => false,
    };
    if !active_member {
        return Some("Передать владение можно только действующему участнику этого чата.".into());
    }
    let mut tx = match db.begin().await {
        Ok(tx) => tx,
        Err(_) => return Some("Не удалось начать передачу владения.".into()),
    };
    let changed = sqlx::query(
        "UPDATE bot_chat_settings SET owner_telegram_id=$2,updated_at=now() WHERE chat_id=$1 AND is_active=TRUE"
    ).bind(chat_id.0).bind(target_id).execute(&mut *tx).await;
    if !matches!(changed, Ok(ref result) if result.rows_affected()==1) {
        let _ = tx.rollback().await;
        return Some("Не удалось передать владение.".into());
    }
    let _ = sqlx::query(
        "INSERT INTO bot_chat_audit_logs(chat_id,actor_telegram_id,target_telegram_id,action,details) VALUES($1,$2,$3,'ownership_transferred',jsonb_build_object('source','telegram_command'))"
    ).bind(chat_id.0).bind(actor).bind(target_id).execute(&mut *tx).await;
    if tx.commit().await.is_err() {
        return Some("Передача владения не была сохранена.".into());
    }
    Some(format!("👑 Владение SMART CAMPUS передано участнику @{} (ID {}).",target,target_id))
}

