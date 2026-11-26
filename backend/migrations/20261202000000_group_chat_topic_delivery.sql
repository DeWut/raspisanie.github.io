-- Telegram forum topic routing and durable delivery state for daily schedule posts.
-- Existing daily-post rows represent posts that were successfully sent by the previous worker.
ALTER TABLE bot_chat_settings
    ADD COLUMN IF NOT EXISTS message_thread_id BIGINT;

ALTER TABLE bot_chat_daily_posts
    ADD COLUMN IF NOT EXISTS delivered_at TIMESTAMPTZ;

UPDATE bot_chat_daily_posts
SET delivered_at = sent_at
WHERE delivered_at IS NULL;
