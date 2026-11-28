const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8');
const html = read('miniapp/index.html');
const app = read('miniapp/app.js');
const api = read('backend/src/handlers/miniapp.rs');
const routes = read('backend/src/handlers/mod.rs');
const bot = read('backend/src/bot.rs');
const migration = read('backend/migrations/20261210000000_miniapp_control_center.sql');
const reminderMigration = read('backend/migrations/20261211000000_group_reminder_controls.sql');
const config = read('miniapp/campus-map-config.js');

const requiredIds = [
  'control-api-note',
  'settings-reminder-minutes',
  'settings-digest-time',
  'managed-chat-select',
  'managed-chat-empty',
  'managed-chat-readonly',
  'managed-chat-editor',
  'managed-chat-group',
  'chat-setting-daily-schedule',
  'chat-setting-digest',
  'chat-setting-lesson-notifications',
  'chat-setting-homework-reminders',
  'chat-setting-exam-reminders',
  'chat-setting-event-reminders',
  'chat-setting-autodz',
  'chat-setting-approver',
  'chat-setting-permission-homework',
  'chat-setting-permission-exams',
  'chat-setting-permission-events',
  'chat-setting-context',
  'chat-setting-consent',
  'group-command-form',
  'assistant-form',
  'notification-worker-status',
  'notification-test',
];

for (const id of requiredIds) {
  assert.match(html, new RegExp('id="' + id + '"'), 'Missing Mini App control: ' + id);
}

for (const tab of ['personal', 'groups', 'tools', 'assistant', 'diagnostics']) {
  assert.match(html, new RegExp('data-control-tab="' + tab + '"'), 'Missing control tab: ' + tab);
  assert.match(html, new RegExp('data-control-pane="' + tab + '"'), 'Missing control pane: ' + tab);
}

const apiPaths = [
  '/api/me/preferences',
  '/api/me/chats',
  '/api/me/chats/:chat_id/settings',
  '/api/me/chats/:chat_id/group',
  '/api/me/chats/:chat_id/command',
  '/api/me/assistant',
  '/api/me/notification-health',
  '/api/me/notification-test',
];
for (const route of apiPaths) {
  assert.ok(routes.includes(route), 'Route not registered: ' + route);
}

for (const endpoint of [
  '/api/me/preferences',
  '/api/me/chats',
  '/settings',
  '/group',
  '/command',
  '/api/me/assistant',
  '/api/me/notification-health',
  '/api/me/notification-test',
]) {
  assert.ok(app.includes(endpoint), 'Mini App does not call expected endpoint/route: ' + endpoint);
}

assert.match(api, /x-telegram-init-data|telegram_user_id/);
assert.match(api, /sync_member\(/, 'Group commands must verify current Telegram membership');
assert.match(api, /can_manage_chat\(/, 'Changing group settings must require administrator rights');
assert.match(api, /consent_context/);
assert.match(api, /content_permissions/);
assert.match(api, /let mut engine = "local_rules"/, 'The assistant must default to deterministic local rules');
assert.match(api, /"engine": engine/, 'The API must report which assistant engine actually handled the request');
assert.match(api, /bot_user_assistant_messages/);

assert.match(bot, /bot_chat_daily_deliveries/);
assert.match(bot, /group_daily_digest/);
assert.match(bot, /personal_daily_digest/);
assert.match(bot, /group_homework_reminder/);
assert.match(bot, /group_exam_reminder/);
assert.match(bot, /personal_event_reminder/);
assert.match(bot, /homework_reminders_enabled/);
assert.match(bot, /exam_reminders_enabled/);
assert.match(bot, /event_reminders_enabled/);
assert.match(api, /homework_reminders_enabled/);
assert.match(api, /exam_reminders_enabled/);
assert.match(api, /event_reminders_enabled/);
assert.match(reminderMigration, /homework_reminders_enabled/);
assert.match(reminderMigration, /exam_reminders_enabled/);
assert.match(reminderMigration, /event_reminders_enabled/);
assert.match(bot, /last_tick_at/);
assert.match(bot, /last_error/);
assert.match(migration, /bot_notification_delivery_log/);
assert.match(migration, /bot_notification_worker_state/);
assert.match(migration, /bot_chat_daily_deliveries/);
assert.match(config, /SMART_CAMPUS_API_URL/);

console.log('Mini App control-center contract: ' + requiredIds.length + ' controls, ' +
  apiPaths.length + ' API routes and notification/context contracts OK.');
