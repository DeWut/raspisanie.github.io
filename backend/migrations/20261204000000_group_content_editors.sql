-- Explicit per-chat, per-section editor allowlists.
CREATE TABLE IF NOT EXISTS bot_chat_content_editors (
    chat_id BIGINT NOT NULL REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    section TEXT NOT NULL CHECK (section IN ('homework','exams','events')),
    telegram_id BIGINT NOT NULL,
    assigned_by BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, section, telegram_id)
);
CREATE INDEX IF NOT EXISTS idx_bot_chat_content_editors_user
    ON bot_chat_content_editors(telegram_id, chat_id);
