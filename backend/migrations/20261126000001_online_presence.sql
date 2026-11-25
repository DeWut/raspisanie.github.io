CREATE TABLE IF NOT EXISTS online_presence (
    session_id UUID PRIMARY KEY,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_online_presence_last_seen
    ON online_presence (last_seen_at);
