-- Telegram bot delivery and reminder bookkeeping.
CREATE TABLE IF NOT EXISTS bot_messages (
    id BIGSERIAL PRIMARY KEY,
    telegram_id BIGINT NOT NULL,
    chat_id BIGINT NOT NULL,
    message_id INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_bot_messages_user_created ON bot_messages(telegram_id,created_at DESC);
CREATE TABLE IF NOT EXISTS bot_reminder_deliveries (
    telegram_id BIGINT NOT NULL,
    lesson_id UUID NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    sent_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(telegram_id,lesson_id)
);
