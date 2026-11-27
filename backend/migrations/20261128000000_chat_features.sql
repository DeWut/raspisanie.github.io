-- Opt-in group assistant context. Each chat has an isolated bounded context.
ALTER TABLE bot_chat_settings
    ADD COLUMN IF NOT EXISTS assistant_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS context_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS context_retention_days SMALLINT NOT NULL DEFAULT 7,
    ADD COLUMN IF NOT EXISTS daily_digest_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS daily_digest_time TIME NOT NULL DEFAULT TIME '07:00';

ALTER TABLE bot_chat_settings
    DROP CONSTRAINT IF EXISTS bot_chat_settings_context_retention_days_check;
ALTER TABLE bot_chat_settings
    ADD CONSTRAINT bot_chat_settings_context_retention_days_check
    CHECK (context_retention_days BETWEEN 1 AND 30);

CREATE TABLE IF NOT EXISTS bot_chat_context_messages (
    id BIGSERIAL PRIMARY KEY,
    chat_id BIGINT NOT NULL REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    message_id BIGINT NOT NULL,
    reply_to_message_id BIGINT,
    sender_telegram_id BIGINT,
    sender_role TEXT NOT NULL DEFAULT 'member',
    message_kind TEXT NOT NULL DEFAULT 'text',
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(chat_id, message_id)
);
CREATE INDEX IF NOT EXISTS idx_bot_chat_context_recent
    ON bot_chat_context_messages(chat_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_bot_chat_context_reply
    ON bot_chat_context_messages(chat_id, reply_to_message_id);

CREATE TABLE IF NOT EXISTS bot_chat_assistant_settings_audit (
    id BIGSERIAL PRIMARY KEY,
    chat_id BIGINT NOT NULL REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    changed_by BIGINT NOT NULL,
    setting TEXT NOT NULL,
    value JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
