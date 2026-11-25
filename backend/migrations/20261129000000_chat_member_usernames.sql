-- Cache Telegram usernames seen in each configured group so /role can resolve @username.
ALTER TABLE bot_chat_members
    ADD COLUMN IF NOT EXISTS username TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_bot_chat_members_username
    ON bot_chat_members (chat_id, lower(username))
    WHERE username IS NOT NULL;
