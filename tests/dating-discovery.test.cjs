const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8');
const social = read('backend/src/services/student_social.rs');
const bot = read('backend/src/bot.rs');
const migration = read('backend/migrations/20261212000000_dating_discovery_preferences.sql');
const docs = read('docs/deployment-miniapp.md');

for (const action of [
  'dating:gender:male',
  'dating:gender:female',
  'dating:pref:male',
  'dating:pref:female',
  'dating:pref:any',
  'dating:repeat',
  'dating:prev',
  'dating:next',
]) {
  assert.ok(social.includes(action), 'Missing dating action: ' + action);
}

assert.match(social, /preferred_gender='any' OR p\.gender=own\.preferred_gender/);
assert.match(social, /repeat_profiles_enabled=TRUE/);
assert.match(social, /random\(\) < 0\.01/);
assert.match(social, /own\.repeat_profiles_enabled=FALSE OR NOT draw\.repeat_draw/, 'Disabling repeats must not suppress unseen profiles');
assert.match(social, /ORDER BY random\(\) LIMIT 1/);
assert.match(social, /bot_dating_seen_profiles/);
assert.match(social, /bot_dating_browse_state/);
assert.match(social, /previous_candidate\(db, actor\)/);
assert.match(social, /displayed_photo/);

// Inline-mode edit callbacks must link into the bot's private chat and open
// the tracked editor there, rather than leaving the user on a generic notice.
assert.match(social, /start=dating_edit/);
assert.match(social, /InlineKeyboardButton::url\("✏️ Открыть редактор", url\)/);
assert.match(social, /pub async fn open_dating_editor/);
assert.match(bot, /argument == "dating_edit"/);
assert.match(bot, /argument == "dating_home"/);
assert.match(social, /render_tracked_dating_screen/);
const inlineDatingBranch = social.slice(
  social.indexOf('if action.starts_with("dating:")', social.indexOf('pub async fn handle_callback')),
  social.indexOf('// Non-dating inline actions remain handled', social.indexOf('pub async fn handle_callback')),
);
assert.match(inlineDatingBranch, /dating_action\(/);
assert.match(inlineDatingBranch, /render_tracked_dating_screen\(/);
assert.doesNotMatch(inlineDatingBranch, /Открой личный чат|Открыть редактор|start=dating_edit/,
  'Inline dating callbacks must operate in the current user private chat without a handoff');
assert.match(social, /Описание: \{\}/);
assert.match(social, /Фото: \{\}/);
assert.match(social, /message to edit not found/);
assert.doesNotMatch(social, /edit_message_text\(chat_id, message_id, "💚 МОЯ АНКЕТА"\)/,
  'Opening the editor must never wipe the existing screen with a placeholder');

assert.match(migration, /gender IN \('male', 'female'\)/);
assert.match(migration, /preferred_gender IN \('male', 'female', 'any'\)/);
assert.match(migration, /repeat_profiles_enabled BOOLEAN NOT NULL DEFAULT FALSE/);
assert.match(migration, /INSERT INTO bot_dating_seen_profiles/);
assert.match(migration, /FROM bot_dating_swipes/);

const photoBlock = social.slice(social.indexOf('// Display a candidate\'s optional Telegram-hosted photo'), social.indexOf('if let Some(liker_text)'));
assert.doesNotMatch(photoBlock, /next_candidate\(/, 'Photo display must use the exact card that was rendered, not fetch another candidate');

assert.doesNotMatch(docs, /Render Dashboard|render\.yaml|onrender\.com/i, 'Deployment guide must not instruct users to use Render');
assert.equal(fs.existsSync(path.join(root, 'render.yaml')), false, 'Render Blueprint should be removed');

console.log('Dating discovery contract: gender, random order, 1/100 repeat, previous/next and photo-card consistency OK.');
