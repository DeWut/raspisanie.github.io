-- Keep Telegram privileges in telegram_status; role is reserved for SMART CAMPUS roles.
-- Existing owner/admin values were mirrored into role by the first implementation.
UPDATE bot_chat_members
SET role = 'member', updated_at = now()
WHERE role IN ('owner', 'admin');
