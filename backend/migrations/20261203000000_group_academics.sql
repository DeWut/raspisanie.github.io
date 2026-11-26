-- Group-scoped homework, exams, events, participation and per-section permissions.
-- IDs are allocated independently per Telegram chat and per record type.

ALTER TABLE bot_chat_settings
    ADD COLUMN IF NOT EXISTS homework_next_id BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS exam_next_id BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS event_next_id BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS timezone TEXT NOT NULL DEFAULT 'Europe/Moscow',
    ADD COLUMN IF NOT EXISTS language TEXT NOT NULL DEFAULT 'ru',
    ADD COLUMN IF NOT EXISTS memes_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS content_permissions JSONB NOT NULL DEFAULT
        '{"homework":"everyone","exams":"everyone","events":"everyone"}'::jsonb;

CREATE TABLE IF NOT EXISTS bot_homework (
    chat_id BIGINT NOT NULL REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    id BIGINT NOT NULL,
    subject TEXT NOT NULL CHECK (length(trim(subject)) BETWEEN 1 AND 160),
    description TEXT NOT NULL CHECK (length(trim(description)) BETWEEN 1 AND 8000),
    due_at TIMESTAMPTZ,
    due_lesson_id UUID REFERENCES lessons(id) ON DELETE SET NULL,
    due_lesson_at TIMESTAMPTZ,
    priority TEXT NOT NULL DEFAULT 'normal' CHECK (priority IN ('normal','urgent')),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','archived','cancelled')),
    created_by BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, id)
);
CREATE INDEX IF NOT EXISTS idx_bot_homework_active_due
    ON bot_homework(chat_id, due_at) WHERE status = 'active';

CREATE TABLE IF NOT EXISTS bot_homework_progress (
    chat_id BIGINT NOT NULL,
    homework_id BIGINT NOT NULL,
    telegram_id BIGINT NOT NULL,
    completed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, homework_id, telegram_id),
    FOREIGN KEY (chat_id, homework_id)
        REFERENCES bot_homework(chat_id, id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_bot_homework_progress_user
    ON bot_homework_progress(chat_id, telegram_id);

CREATE TABLE IF NOT EXISTS bot_exams (
    chat_id BIGINT NOT NULL REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    id BIGINT NOT NULL,
    subject TEXT NOT NULL CHECK (length(trim(subject)) BETWEEN 1 AND 160),
    exam_at TIMESTAMPTZ NOT NULL,
    room TEXT,
    format TEXT,
    details TEXT,
    topics TEXT,
    literature TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','archived','cancelled')),
    created_by BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, id)
);
CREATE INDEX IF NOT EXISTS idx_bot_exams_active_date
    ON bot_exams(chat_id, exam_at) WHERE status = 'active';

CREATE TABLE IF NOT EXISTS bot_events (
    chat_id BIGINT NOT NULL REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    id BIGINT NOT NULL,
    title TEXT NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 200),
    starts_at TIMESTAMPTZ NOT NULL,
    location TEXT,
    description TEXT,
    capacity INTEGER CHECK (capacity IS NULL OR capacity > 0),
    waitlist_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','cancelled','completed')),
    created_by BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, id)
);
CREATE INDEX IF NOT EXISTS idx_bot_events_active_date
    ON bot_events(chat_id, starts_at) WHERE status = 'active';

CREATE TABLE IF NOT EXISTS bot_event_participants (
    chat_id BIGINT NOT NULL,
    event_id BIGINT NOT NULL,
    telegram_id BIGINT NOT NULL,
    response TEXT NOT NULL DEFAULT 'going' CHECK (response IN ('going','not_going','maybe','waitlist')),
    reminders_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, event_id, telegram_id),
    FOREIGN KEY (chat_id, event_id)
        REFERENCES bot_events(chat_id, id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_bot_event_participants_reminders
    ON bot_event_participants(chat_id, event_id, response)
    WHERE reminders_enabled = TRUE;

CREATE TABLE IF NOT EXISTS bot_group_reminder_deliveries (
    chat_id BIGINT NOT NULL REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    record_type TEXT NOT NULL CHECK (record_type IN ('homework','exam','event')),
    record_id BIGINT NOT NULL,
    telegram_id BIGINT NOT NULL,
    reminder_key TEXT NOT NULL,
    sent_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, record_type, record_id, telegram_id, reminder_key)
);

CREATE TABLE IF NOT EXISTS bot_group_content_audit (
    id BIGSERIAL PRIMARY KEY,
    chat_id BIGINT NOT NULL REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    actor_telegram_id BIGINT NOT NULL,
    action TEXT NOT NULL,
    record_type TEXT NOT NULL CHECK (record_type IN ('homework','exam','event','settings','ownership')),
    record_id BIGINT,
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_bot_group_content_audit_chat
    ON bot_group_content_audit(chat_id, created_at DESC);
