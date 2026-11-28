const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const root = path.resolve(__dirname, '..');
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8');
const app = read('miniapp/app.js');
const rootPage = read('index.html');
const miniAppPage = read('miniapp/index.html');
const weekPage = read('miniapp/week.html');

// The shared script runs on three HTML shells. The public root and week page
// intentionally do not contain the optional Mini App home greeting.
const greetingSource = app.match(/function setGreeting\(\) \{[\s\S]*?\n  \}/);
assert.ok(greetingSource, 'setGreeting must exist');

for (const [label, html, includesHomeGreeting] of [
  ['root schedule', rootPage, false],
  ['week schedule', weekPage, false],
  ['full Mini App', miniAppPage, true],
]) {
  const hasHomeGreeting = /id="home-greeting"/.test(html);
  assert.equal(hasHomeGreeting, includesHomeGreeting, label + ' has unexpected greeting markup');

  const pageGreeting = { textContent: '' };
  const homeGreeting = includesHomeGreeting ? { textContent: '' } : null;
  const context = {
    tg: { initDataUnsafe: { user: { first_name: 'Тест' } } },
    $: (selector) => selector === '#greeting'
      ? pageGreeting
      : selector === '#home-greeting' ? homeGreeting : null,
  };
  const setGreeting = vm.runInNewContext('(' + greetingSource[0] + ')', context);
  assert.doesNotThrow(setGreeting, label + ' greeting initialization must not throw');
  assert.equal(pageGreeting.textContent, 'Привет, Тест!');
  if (homeGreeting) assert.equal(homeGreeting.textContent, 'Добро пожаловать, Тест');
}

// Legacy pages do not expose the settings control center. That must never
// turn a successful timetable load into an uncaught account-UI exception.
assert.match(app, /if \(!status \|\| !note \|\| !saveButton\) return;/);
assert.match(app, /const hasAccountSettingsUi = Boolean\(/);
assert.match(app, /if \(!isWeekPage && \$\('#home-view'\)\) showView\('home'\);/);

// Theme setters are only sent to Telegram clients that support them.
assert.match(app, /tg\?\.isVersionAtLeast\?\.\('6\.1'\)/);
assert.match(app, /tg\.isVersionAtLeast\('6\.9'\)/);

console.log('Mini App shell compatibility: root, week and full Mini App startup guards OK.');
