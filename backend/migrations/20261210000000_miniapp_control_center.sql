-- Mini App control center, bounded educational assistant context and notification observability.

CREATE TABLE IF NOT EXISTS bot_user_assistant_messages (
    id BIGSERIAL PRIMARY KEY,
    telegram_id BIGINT NOT NULL REFERENCES users(telegram_id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('user','assistant')),
    content TEXT NOT NULL CHECK (length(content) <= 4000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_bot_user_assistant_recent
    ON bot_user_assistant_messages(telegram_id, id DESC);

CREATE TABLE IF NOT EXISTS bot_notification_worker_state (
    worker TEXT PRIMARY KEY,
    last_tick_at TIMESTAMPTZ,
    last_success_at TIMESTAMPTZ,
    last_error TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS bot_notification_delivery_log (
    id BIGSERIAL PRIMARY KEY,
    kind TEXT NOT NULL,
    recipient_kind TEXT NOT NULL CHECK (recipient_kind IN ('private','group')),
    recipient_id BIGINT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('sent','failed')),
    detail TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_bot_notification_delivery_recent
    ON bot_notification_delivery_log(recipient_kind, recipient_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_bot_notification_delivery_created
    ON bot_notification_delivery_log(created_at DESC);

-- The old daily-post ledger allowed only one message per chat and date.
-- Use a separate ledger so a schedule post and a digest can be sent independently.
CREATE TABLE IF NOT EXISTS bot_chat_daily_deliveries (
    chat_id BIGINT NOT NULL REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    local_date DATE NOT NULL,
    post_kind TEXT NOT NULL CHECK (post_kind IN ('schedule','digest')),
    sent_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at TIMESTAMPTZ,
    PRIMARY KEY (chat_id, local_date, post_kind)
);

-- Group-level switches for deadline, exam and event reminders.
ALTER TABLE bot_chat_settings
    ADD COLUMN IF NOT EXISTS homework_reminders_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS exam_reminders_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS event_reminders_enabled BOOLEAN NOT NULL DEFAULT TRUE;
