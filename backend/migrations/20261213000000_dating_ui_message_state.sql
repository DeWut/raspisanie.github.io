-- Keep exactly one editable dating UI message per Telegram user.
-- Only this bot-owned screen is managed; arbitrary chat messages are never swept.
CREATE TABLE IF NOT EXISTS bot_dating_ui_messages (
    telegram_id BIGINT PRIMARY KEY,
    chat_id BIGINT NOT NULL,
    message_id BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_bot_dating_ui_messages_chat
    ON bot_dating_ui_messages(chat_id);
