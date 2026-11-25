
-- Settings for Telegram group and supergroup chats.
-- Each Telegram chat can be linked to one academic group.

CREATE TABLE IF NOT EXISTS bot_chat_settings (
    chat_id BIGINT PRIMARY KEY,
    group_id UUID NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    chat_title TEXT,
    configured_by BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_bot_chat_settings_group
    ON bot_chat_settings(group_id);
