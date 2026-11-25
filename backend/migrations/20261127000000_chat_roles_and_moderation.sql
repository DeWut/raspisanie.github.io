-- Per-chat SMART CAMPUS roles, moderation audit trail and automod settings.
-- Telegram remains the source of truth for chat ownership and administrator status.

ALTER TABLE bot_chat_settings
    ADD COLUMN IF NOT EXISTS owner_telegram_id BIGINT,
    ADD COLUMN IF NOT EXISTS settings JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS is_active BOOLEAN NOT NULL DEFAULT TRUE;

CREATE TABLE IF NOT EXISTS bot_chat_members (
    chat_id BIGINT NOT NULL
        REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    telegram_id BIGINT NOT NULL,
    role TEXT NOT NULL DEFAULT 'member'
        CHECK (role IN (
            'owner',
            'admin',
            'senior_moderator',
            'moderator',
            'helper',
            'member'
        )),
    telegram_status TEXT NOT NULL DEFAULT 'member'
        CHECK (telegram_status IN (
            'creator',
            'administrator',
            'member',
            'restricted',
            'left',
            'kicked'
        )),
    assigned_by BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (chat_id, telegram_id)
);

CREATE INDEX IF NOT EXISTS idx_bot_chat_members_user
    ON bot_chat_members(telegram_id);

CREATE INDEX IF NOT EXISTS idx_bot_chat_members_role
    ON bot_chat_members(chat_id, role);

CREATE TABLE IF NOT EXISTS bot_chat_audit_logs (
    id BIGSERIAL PRIMARY KEY,
    chat_id BIGINT NOT NULL
        REFERENCES bot_chat_settings(chat_id) ON DELETE CASCADE,
    actor_telegram_id BIGINT,
    target_telegram_id BIGINT,
    action TEXT NOT NULL,
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_bot_chat_audit_logs_chat_created
    ON bot_chat_audit_logs(chat_id, created_at DESC);
