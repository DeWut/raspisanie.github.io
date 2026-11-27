-- Per-chat switches for group homework, exam and event reminder delivery.
ALTER TABLE bot_chat_settings
    ADD COLUMN IF NOT EXISTS homework_reminders_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS exam_reminders_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS event_reminders_enabled BOOLEAN NOT NULL DEFAULT TRUE;
