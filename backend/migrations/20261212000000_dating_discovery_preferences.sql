-- Dating discovery preferences and one-card back/forward navigation.
-- Existing profile records remain valid: gender is optional until the owner selects it.

ALTER TABLE bot_dating_profiles
    ADD COLUMN IF NOT EXISTS gender TEXT
        CHECK (gender IS NULL OR gender IN ('male', 'female')),
    ADD COLUMN IF NOT EXISTS preferred_gender TEXT NOT NULL DEFAULT 'any'
        CHECK (preferred_gender IN ('male', 'female', 'any')),
    ADD COLUMN IF NOT EXISTS repeat_profiles_enabled BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS bot_dating_seen_profiles (
    viewer_telegram_id BIGINT NOT NULL REFERENCES bot_dating_profiles(telegram_id) ON DELETE CASCADE,
    viewed_telegram_id BIGINT NOT NULL REFERENCES bot_dating_profiles(telegram_id) ON DELETE CASCADE,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (viewer_telegram_id, viewed_telegram_id),
    CHECK (viewer_telegram_id <> viewed_telegram_id)
);

CREATE INDEX IF NOT EXISTS idx_bot_dating_seen_recent
    ON bot_dating_seen_profiles(viewer_telegram_id, last_seen_at DESC);

CREATE TABLE IF NOT EXISTS bot_dating_browse_state (
    viewer_telegram_id BIGINT PRIMARY KEY REFERENCES bot_dating_profiles(telegram_id) ON DELETE CASCADE,
    current_profile_id BIGINT REFERENCES bot_dating_profiles(telegram_id) ON DELETE SET NULL,
    previous_profile_id BIGINT REFERENCES bot_dating_profiles(telegram_id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (current_profile_id IS NULL OR current_profile_id <> viewer_telegram_id),
    CHECK (previous_profile_id IS NULL OR previous_profile_id <> viewer_telegram_id)
);

-- Preserve the existing history so old dislikes/passes can be revisited.
INSERT INTO bot_dating_seen_profiles(viewer_telegram_id, viewed_telegram_id, last_seen_at)
SELECT from_telegram_id, to_telegram_id, max(created_at)
FROM bot_dating_swipes
GROUP BY from_telegram_id, to_telegram_id
ON CONFLICT(viewer_telegram_id, viewed_telegram_id) DO NOTHING;
