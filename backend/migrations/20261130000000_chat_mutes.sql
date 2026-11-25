-- Store the Telegram permission set that existed before a timed SMART CAMPUS mute.
-- This lets /unmute restore the chat defaults instead of granting every permission.
CREATE TABLE IF NOT EXISTS bot_chat_mutes (
    chat_id BIGINT NOT NULL
        REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    target_telegram_id BIGINT NOT NULL,
    actor_telegram_id BIGINT NOT NULL,
    original_permissions JSONB NOT NULL,
    muted_until TIMESTAMPTZ NOT NULL,
    reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, target_telegram_id)
);

CREATE INDEX IF NOT EXISTS idx_bot_chat_mutes_until
    ON bot_chat_mutes(chat_id, muted_until);
