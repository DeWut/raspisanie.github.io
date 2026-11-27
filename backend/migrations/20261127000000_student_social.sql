-- Dating inbox and profile media.
-- Telegram file_id is stored, not a public URL; the bot controls when it is sent.
ALTER TABLE bot_dating_profiles
    ADD COLUMN IF NOT EXISTS photo_file_id TEXT;

CREATE INDEX IF NOT EXISTS idx_bot_dating_swipes_incoming_likes
    ON bot_dating_swipes(to_telegram_id, created_at DESC)
    WHERE action = 'like';

CREATE TABLE IF NOT EXISTS bot_dating_inbox_state (
    telegram_id BIGINT PRIMARY KEY REFERENCES bot_dating_profiles(telegram_id) ON DELETE CASCADE,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_viewed_from_telegram_id BIGINT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
