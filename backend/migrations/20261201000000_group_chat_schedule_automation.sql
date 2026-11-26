-- Opt-in daily schedule posts and lesson reminders for configured Telegram chats.
ALTER TABLE bot_chat_settings
    ADD COLUMN IF NOT EXISTS daily_schedule_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS daily_schedule_time TIME NOT NULL DEFAULT '07:00',
    ADD COLUMN IF NOT EXISTS lesson_notifications_enabled BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS bot_chat_daily_posts (
    chat_id BIGINT NOT NULL REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    local_date DATE NOT NULL,
    sent_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, local_date)
);

CREATE TABLE IF NOT EXISTS bot_chat_lesson_notifications (
    chat_id BIGINT NOT NULL REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    lesson_id UUID NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    sent_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, lesson_id)
);

CREATE INDEX IF NOT EXISTS idx_bot_chat_settings_daily_schedule
    ON bot_chat_settings(daily_schedule_time)
    WHERE daily_schedule_enabled = TRUE;

CREATE INDEX IF NOT EXISTS idx_bot_chat_settings_lesson_notifications
    ON bot_chat_settings(chat_id)
    WHERE lesson_notifications_enabled = TRUE;
