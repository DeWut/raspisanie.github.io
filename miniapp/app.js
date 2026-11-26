(() => {
  'use strict';

  const DAYS = [
    { id: 1, short: 'ПН', full: 'Понедельник' },
    { id: 2, short: 'ВТ', full: 'Вторник' },
    { id: 3, short: 'СР', full: 'Среда' },
    { id: 4, short: 'ЧТ', full: 'Четверг' },
    { id: 5, short: 'ПТ', full: 'Пятница' },
    { id: 6, short: 'СБ', full: 'Суббота' },
    { id: 7, short: 'ВС', full: 'Воскресенье' },
  ];
  // Keep this anchor in sync with the semester week numbering used by the existing timetable.
  const TIME_ZONE = 'Europe/Moscow';
  const SEMESTER_START = new Date(Date.UTC(2026, 7, 31, 12));
  const GROUP_KEY = 'smart-campus-group-id';
  const THEME_KEY = 'spbftuit-theme';
  const NOTIFICATIONS_KEY = 'spbftuit-lesson-notifications';
  const pageParams = new URLSearchParams(location.search);
  const isWeekPage = document.body.dataset.page === 'week';
  const requestedDate = pageParams.get('date');
  const parsedRequestedDate = requestedDate && /^\d{4}-\d{2}-\d{2}$/.test(requestedDate) ? new Date(requestedDate + 'T12:00:00Z') : null;
  const state = { groups: [], schedule: null, selectedDate: parsedRequestedDate && !Number.isNaN(parsedRequestedDate.getTime()) ? parsedRequestedDate : startOfDay(new Date()), view: isWeekPage ? 'week' : 'today', searchQuery: '', filter: 'current', calendarMonth: new Date().getMonth(), calendarYear: new Date().getFullYear(), campus: null };
  const $ = (selector) => document.querySelector(selector);
  const tg = window.Telegram?.WebApp;
  // Resolve data relative to the app script, not the page URL.
  const APP_SCRIPT_URL = document.currentScript?.src || new URL('miniapp/app.js', location.href).href;
  const DATA_ROOT = new URL('../data/', APP_SCRIPT_URL).href;
  const DATA_CACHE_NAME = 'spbftuit-data-v1';
  const DATA_FALLBACKS = [
    'https://cdn.jsdelivr.net/gh/DeWut/raspisanie.github.io@main/data/',
    'https://raw.githubusercontent.com/DeWut/raspisanie.github.io/main/data/',
  ];
  async function cacheDataResponse(url, response) {
    if (!('caches' in window)) return;
    try { const cache = await caches.open(DATA_CACHE_NAME); await cache.put(url, response.clone()); }
    catch (error) { console.debug('Offline data cache is unavailable:', error); }
  }
  async function readCachedData(url) {
    if (!('caches' in window)) return null;
    try { const cache = await caches.open(DATA_CACHE_NAME); return await cache.match(url); }
    catch { return null; }
  }
  async function fetchDataFile(path) {
    const localUrl = new URL(path, DATA_ROOT).href;
    // Do not wait for external mirrors while the browser already reports offline.
    if (!navigator.onLine) {
      const cached = await readCachedData(localUrl);
      if (cached) return cached;
    }
    const candidates = [...new Set([localUrl, ...DATA_FALLBACKS.map((root) => new URL(path, root).href)])];
    let lastError;
    for (const url of candidates) {
      try {
        const response = await fetch(url, { cache: 'no-cache' });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        await cacheDataResponse(localUrl, response);
        return response;
      } catch (error) { lastError = error; }
    }
    const cached = await readCachedData(localUrl);
    if (cached) return cached;
    throw lastError || new Error('data-unavailable');
  }
  function registerOfflineSupport() {
    if (!('serviceWorker' in navigator) || !/^(https?:|capacitor:)/.test(location.protocol)) return;
    navigator.serviceWorker.register(new URL('../service-worker.js', APP_SCRIPT_URL), { updateViaCache: 'none' })
      .catch((error) => console.warn('Offline shell registration failed:', error));
  }
  function updateConnectivityState() {
    const note = $('#connectivity-note');
    if (note) note.hidden = navigator.onLine;
    if (state.schedule) renderHero();
  }
  const PRESENCE_API = (window.SMART_CAMPUS_API_URL || document.querySelector('meta[name="smart-campus-api"]')?.content || '').replace(new RegExp('/+$'), '');
  function presenceSessionId() {
    const key = 'spbftuit-presence-session';
    let id = safeStorageGet(key);
    if (!id) {
      if (window.crypto?.randomUUID) id = window.crypto.randomUUID();
      else {
        const bytes = new Uint8Array(16);
        window.crypto.getRandomValues(bytes);
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        id = [...bytes].map((b, i) => ([4, 6, 8, 10].includes(i) ? '-' : '') + b.toString(16).padStart(2, '0')).join('');
      }
      safeStorageSet(key, id);
    }
    return id;
  }
  async function updatePresence() {
    if (!PRESENCE_API) return;
    try {
      const pingResponse = await fetch(new URL('/api/presence/ping', PRESENCE_API), {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ session_id: presenceSessionId() }), cache: 'no-store',
      });
      if (!pingResponse.ok) throw new Error('presence-ping-failed');
      const response = await fetch(new URL('/api/presence', PRESENCE_API), { cache: 'no-store' });
      if (!response.ok) throw new Error('presence-unavailable');
      const data = await response.json();
      if (!Number.isSafeInteger(data.online) || data.online < 0) throw new Error('invalid-presence');
      $('#online-count').textContent = String(data.online);
      $('#online-indicator').hidden = false;
    } catch {
      $('#online-indicator').hidden = true;
    }
  }
  const teachersDB = {
        "Васильев Н.П.":     { name: "Васильев Николай Павлович",       url: "https://spbftu.ru/allchairs/chairs-informatsionnykh-sistem-i-tekhnologiy/vasilev-nikolay-pavlovich" },
        "Мельников Н.Н.":    { name: "Мельников Николай Николаевич",    url: "https://spbftu.ru/allchairs/chairs-filosofii-i-sotsialnykh-distsiplin/melnikov-nikolay-nikolaevich" },
        "Вохмянин Н.А.":     { name: "Вохмянин Николай Александрович",  url: "https://spbftu.ru/allchairs/chairs-prikladnoy-mekhaniki-i-inzhenernoy-grafiki/vokhmyanin-nikolay-aleksandrovich" },
        "Совтус Н.В.":       { name: "Совтус Никита Владимирович", url: "https://spbftu.ru/allchairs/chairs-fiziki/sovtus-nikita-vladimirovich" },
        "Куликова Д.Р.":     { name: "Куликова Дарья Романовна",        url: "https://spbftu.ru/allchairs/chairs-prikladnoy-mekhaniki-i-inzhenernoy-grafiki/kulikova-darya-romanovna" },
        "Подолянец Л.А.":    { name: "Подолянец Лада Авенировна",       url: "https://spbftu.ru/allchairs/chairs-ekonomiki-ucheta-i-analiza-khozyaystvennoy-deyatelnosti/podolyanets-lada-avenirovna" },
        "Тарасеня Т.Ю.":     { name: "Тарасеня Татьяна Юрьевна",        url: "https://spbftu.ru/allchairs/chairs-fizicheskoy-kultury/tarasenya-tatyana-yurevna" },
        "Лешева Н.С.":       { name: "Лешева Наталья Сергеевна",        url: "https://spbftu.ru/allchairs/chairs-fizicheskoy-kultury/lesheva-natalya-sergeevna" },
        "Ломакина И.С.":     { name: "Ломакина Ирина Сергеевна",        url: "https://spbftu.ru/allchairs/chairs-inostrannykh-yazykov/lomakina-irina-sergeevna" },
        "Михайлова А.И.":    { name: "Михайлова Анна Ивановна",         url: "https://spbftu.ru/allchairs/chairs-russkogo-yazyka/mikhaylova-anna-ivanovna" },
        "Былев А.Б.":        { name: "Былев Александр Борисович",       url: "https://spbftu.ru/allchairs/chairs-fiziki/bylev-aleksandr-borisovich" },
        "Одинцова Н.А.":     { name: "Одинцова Н.А.",                   url: null },
        "Шлапакова С.Н.":    { name: "Шлапакова Светлана Николаевна",   url: "https://spbftu.ru/university/administration/shlapakova-svetlana-nikolaevna" },
        "Басова Е.Н.":       { name: "Басова Елизавета Николаевна",     url: "https://spbftu.ru/allchairs/chairs-russkogo-yazyka/basova-elizaveta-nikolaevna" },
        "Гущина Е.В.":       { name: "Гущина Екатерина Владимировна",   url: "https://spbftu.ru/allchairs/chairs-fiziki/gushchina-ekaterina-vladimirovna" },
        "Мартын К.А.":       { name: "Мартын Кристина Андреевна",       url: "https://spbftu.ru/allchairs/chairs-informatsionnykh-sistem-i-tekhnologiy/martyn-kristina-andreevna" },
        "Нуруллина Р.В.":    { name: "Нуруллина Роза Вагизовна",        url: "https://spbftu.ru/allchairs/chairs-filosofii-i-sotsialnykh-distsiplin/nurullina-roza-vagizovna" },
        "Совтус Н.В.":       { name: "Совтус Никита Владимирович",      url: "https://spbftu.ru/allchairs/chairs-fiziki/sovtus-nikita-vladimirovich" },
        "Гаврилов С.П.":     { name: "Гаврилов Сергей Петрович",        url: "https://spbftu.ru/allchairs/chairs-fiziki/gavrilov-sergey-petrovich" },

        /* Преподаватели, встречающиеся в расписании ТМб-СтМет */
        "Григоренко Н.А.":   { name: "Григоренко Наталья Александровна", url: "https://spbftu.ru/allchairs/chairs-russkogo-yazyka/grigorenko-natalya-aleksandrovna?ysclid=mv1dnnmkxq67789909" },
        "Шульгин Е.Д.":      { name: "Шульгин Евгений Дмитриевич",       url: "https://spbftu.ru/employee/viewing/shulgin-evgeniy-dmitrievich?ysclid=mv1dnp6tyo122322443" },
        "Кузько М.В.":       { name: "Кузько Мария Владимировна",        url: "https://spbftu.ru/allchairs/chairs-inostrannykh-yazykov/kuzko-mariya-viktorovna?ysclid=mv1do9qc6n311447681" },
        "Вернер Н.Н.":       { name: "Вернер Наталья Николаевна",        url: "https://spbftu.ru/employee/viewing/verner-nadezhda-nikolaevna?ysclid=mv1dob843q550948177" }
  };

  async function loadTeacherDirectory() {
    try {
      const response = await fetchDataFile('teachers.json');
      const directory = await response.json();
      if (!directory || typeof directory !== 'object' || Array.isArray(directory)) return;
      for (const [shortName, profile] of Object.entries(directory)) {
        if (profile && typeof profile.name === 'string' && typeof profile.url === 'string' && /^https:\/\/spbftu\.ru\//.test(profile.url)) {
          teachersDB[shortName] = profile;
        }
      }
    } catch { /* The hand-maintained directory remains available if the university site is temporarily unavailable. */ }
  }
  function moscowDateParts(date = new Date()) {
    return Object.fromEntries(new Intl.DateTimeFormat('en-CA', {
      timeZone: TIME_ZONE, year: 'numeric', month: '2-digit', day: '2-digit',
      hour: '2-digit', minute: '2-digit', hourCycle: 'h23',
    }).formatToParts(date).map((part) => [part.type, part.value]));
  }
  function startOfDay(date) { const p = moscowDateParts(date); return new Date(Date.UTC(Number(p.year), Number(p.month) - 1, Number(p.day), 12)); }
  function dateKey(date) { return `${date.getUTCFullYear()}-${String(date.getUTCMonth() + 1).padStart(2, '0')}-${String(date.getUTCDate()).padStart(2, '0')}`; }
  function dayNumber(date) { const day = date.getUTCDay(); return day === 0 ? 7 : day; }
  function weekStart(date) { const value = startOfDay(date); value.setUTCDate(value.getUTCDate() - dayNumber(value) + 1); return value; }
  function isOddWeek(date) {
    const start = startOfDay(SEMESTER_START);
    const diff = Math.floor((startOfDay(date) - start) / 86_400_000);
    return Math.floor(diff / 7) % 2 === 0;
  }
  function formatDate(date, options = { day: 'numeric', month: 'long' }) {
    return new Intl.DateTimeFormat('ru-RU', { ...options, timeZone: TIME_ZONE }).format(date);
  }
  function safeStorageGet(key) { try { return localStorage.getItem(key); } catch { return null; } }
  function safeStorageSet(key, value) { try { localStorage.setItem(key, value); } catch { /* Telegram may disable storage in private mode. */ } }
  function reminderDateTime(date, time) {
    const [year, month, day] = dateKey(date).split('-').map(Number);
    const [hour, minute] = String(time).split(':').map(Number);
    // EIOS timetable is in Saint Petersburg/Moscow time (UTC+3), independent of phone timezone.
    return new Date(Date.UTC(year, month - 1, day, hour, minute) - 3 * 60 * 60 * 1000);
  }
  function reminderId(date, index) {
    return Number(dateKey(date).replaceAll('-', '')) * 100 + index;
  }
  function buildLessonReminders() {
    if (!state.schedule?.id) return [];
    const items = [];
    const today = startOfDay(new Date());
    for (let offset = 0; offset < 14; offset += 1) {
      const date = new Date(today);
      date.setUTCDate(date.getUTCDate() + offset);
      getLessons(date).forEach((lesson, index) => {
        const at = reminderDateTime(date, lesson.start);
        at.setMinutes(at.getMinutes() - 10);
        if (at.getTime() <= Date.now() + 60_000) return;
        const room = lesson.room ? ` · ${String(lesson.room).replace(/^ауд\\.\\s*/i, '')}` : '';
        items.push({
          id: reminderId(date, index),
          title: 'Скоро пара',
          body: `${lesson.name} · ${lesson.start}${room}`,
          at: at.toISOString(),
          date: dateKey(date),
          groupId: String(state.schedule.id),
        });
      });
    }
    return items;
  }
  function updateNotificationButton() {
    const button = $('#toggle-notifications');
    if (!button) return;
    const enabled = safeStorageGet(NOTIFICATIONS_KEY) === '1';
    button.setAttribute('aria-pressed', String(enabled));
    button.title = enabled ? 'Напоминания включены' : 'Напоминания выключены';
    button.setAttribute('aria-label', button.title);
    button.textContent = enabled ? '🔔' : '♧';
  }
  async function syncLessonReminders() {
    if (safeStorageGet(NOTIFICATIONS_KEY) !== '1' || !state.schedule) return;
    const native = window.SPBFTUITNative;
    if (!native?.available) return;
    try {
      const result = await native.scheduleLessonReminders(buildLessonReminders());
      if (!result.granted) showToast('Разреши уведомления для SPBFTUIT в настройках Android');
    } catch (error) {
      console.error('Could not schedule lesson reminders', error);
      showToast('Не удалось обновить напоминания');
    }
  }
  async function toggleLessonReminders() {
    const native = window.SPBFTUITNative;
    if (!native?.available) {
      showToast('Уведомления доступны в Android-приложении');
      return;
    }
    const enabled = safeStorageGet(NOTIFICATIONS_KEY) === '1';
    if (enabled) {
      await native.cancelLessonReminders();
      safeStorageSet(NOTIFICATIONS_KEY, '0');
      updateNotificationButton();
      showToast('Напоминания отключены');
      return;
    }
    if (!state.schedule) {
      showToast('Сначала выбери учебную группу');
      return;
    }
    try {
      const permission = await native.requestNotificationPermission();
      if (!permission.granted) {
        showToast('Разреши уведомления для SPBFTUIT в Android');
        return;
      }
      safeStorageSet(NOTIFICATIONS_KEY, '1');
      updateNotificationButton();
      const result = await native.scheduleLessonReminders(buildLessonReminders());
      showToast(`Напоминания включены: ${result.scheduled} занятий на 14 дней`);
    } catch (error) {
      safeStorageSet(NOTIFICATIONS_KEY, '0');
      updateNotificationButton();
      console.error('Could not enable lesson reminders', error);
      showToast('Не удалось включить напоминания');
    }
  }
  function applyTheme(theme, persist = false) {
    const next = theme === 'dark' ? 'dark' : 'light';
    document.documentElement.dataset.theme = next;
    document.documentElement.style.colorScheme = next;
    const button = $('#toggle-theme');
    if (button) {
      button.setAttribute('aria-pressed', String(next === 'dark'));
      button.setAttribute('aria-label', next === 'dark' ? 'Включена тёмная тема. Переключить на светлую' : 'Включена светлая тема. Переключить на тёмную');
      button.title = next === 'dark' ? 'Светлая тема' : 'Тёмная тема';
      button.innerHTML = next === 'dark'
        ? '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.42 1.42M17.65 17.65l1.42 1.42M2 12h2M20 12h2M4.93 19.07l1.42-1.42M17.65 6.35l1.42-1.42"/></svg>'
        : '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20.5 15.5A8.5 8.5 0 0 1 8.5 3.5 8.5 8.5 0 1 0 20.5 15.5Z"/></svg>';
    }
    const meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.content = next === 'dark' ? '#101b16' : '#173d2e';
    if (tg) {
      tg.setHeaderColor?.(next === 'dark' ? '#101b16' : '#173d2e');
      tg.setBackgroundColor?.(next === 'dark' ? '#101b16' : '#f5f7f2');
    }
    if (persist) safeStorageSet(THEME_KEY, next);
  }
  function escapeHtml(value) {
    return String(value ?? '').replace(/[&<>"']/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[char]);
  }
  function minutes(value) { const [hour, minute] = String(value || '0:0').split(':').map(Number); return hour * 60 + minute; }
  function getLessons(date, filter = 'current') {
    const lessons = state.schedule?.lessons?.[String(dayNumber(date))] || [];
    const parity = isOddWeek(date) ? 'нечёт' : 'чёт';
    return lessons.filter((lesson) => {
      const name = String(lesson.name || '').trim();
      if (!name || /^(загрузка(?:…|\.\.\.)?|loading(?:…|\.\.\.)?)$/i.test(name)) return false;
      if (!lesson.start || !lesson.end) return false;
      if (filter === 'odd') return lesson.week === '*' || lesson.week === 'нечёт';
      if (filter === 'even') return lesson.week === '*' || lesson.week === 'чёт';
      if (filter === 'current') return lesson.week === '*' || lesson.week === parity;
      return true;
    }).slice().sort((a, b) => minutes(a.start) - minutes(b.start));
  }
  function parseRoom(raw) {
    if (!raw) return '';
    const cleaned = String(raw).replace(/^ауд\.\s*/i, '').trim();
    const match = cleaned.match(/^([^.]+)\.(\d)(\d+)$/);
    if (!match) return cleaned;
    const building = match[1].trim();
    const roomNumber = match[2] + match[3];
    const floorDigit = Number(match[2]);
    const floor = floorDigit <= 1 ? 1 : floorDigit;
    return building + ' корпус, ' + floor + ' этаж, аудитория ' + roomNumber;
  }
  function setGreeting() {
    const name = tg?.initDataUnsafe?.user?.first_name;
    $('#greeting').textContent = name ? `Привет, ${name}!` : 'Привет!';
  }
  function showToast(message) {
    const toast = $('#toast');
    toast.textContent = message;
    toast.classList.add('show');
    window.clearTimeout(showToast.timeout);
    showToast.timeout = window.setTimeout(() => toast.classList.remove('show'), 2300);
  }
  function updateGroupLabel() {
    const selected = state.groups.find((group) => String(group.id) === String(state.schedule?.id));
    $('#group-name').textContent = selected?.name || state.schedule?.name || 'Выбрать группу';
    $('#sync-bot-group').hidden = !tg?.sendData || !state.schedule?.id;
  }
  function lessonType(name) {
    const match = String(name || '').trim().match(/\s+(Лек\.|Пр\.|Лаб\.)$/i);
    if (!match) return '';
    return ({ 'лек.': 'Лекция', 'пр.': 'Практика', 'лаб.': 'Лабораторная' })[match[1].toLowerCase()] || match[1];
  }
  function lessonBaseName(name) {
    return String(name || '').trim().replace(/\s+(Лек\.|Пр\.|Лаб\.)$/i, '').trim();
  }
  function compactLessonTitle(name) {
    const value = String(name || '').trim();
    if (/социально-ознакомительным практикумом/i.test(value) && /парк\s+лта/i.test(value)) {
      return 'Социально-ознакомительный практикум · Парк ЛТА';
    }
    return value.length > 78 ? value.slice(0, 75).trimEnd() + '…' : value;
  }
  function groupLessonsBySlot(lessons) {
    const slots = new Map();
    for (const lesson of lessons) {
      const key = [lesson.pair ?? '', lesson.start || '', lesson.end || ''].join('|');
      if (!slots.has(key)) slots.set(key, {
        pair: lesson.pair,
        start: lesson.start,
        end: lesson.end,
        entries: [],
        week: lesson.week || '*',
      });
      const slot = slots.get(key);
      const entryKey = [lesson.name || '', lesson.teacher || ''].join('|');
      let entry = slot.entries.find((item) => item.key === entryKey);
      if (!entry) {
        entry = { key: entryKey, name: lesson.name || 'Занятие', teacher: lesson.teacher || '', rooms: [], week: lesson.week || '*' };
        slot.entries.push(entry);
      }
      const room = String(lesson.room || '').trim();
      if (room && !entry.rooms.includes(room)) entry.rooms.push(room);
      if (lesson.week && lesson.week !== '*' && !slot.week) slot.week = lesson.week;
    }
    return [...slots.values()].sort((a, b) => minutes(a.start) - minutes(b.start));
  }
  function slotTitle(slot) {
    if (slot.entries.length === 1) return compactLessonTitle(slot.entries[0].name);
    const bases = slot.entries.map((entry) => lessonBaseName(entry.name));
    return bases.every((name) => name === bases[0]) ? compactLessonTitle(bases[0]) : 'Несколько занятий';
  }
  function formatEntryRooms(entry) {
    const rooms = entry.rooms.map(parseRoom);
    if (rooms.length > 1) {
      const parsed = rooms.map((room) => room.match(/^(.*), аудитория (.+)$/));
      if (parsed.every(Boolean) && parsed.every((item) => item[1] === parsed[0][1])) {
        return parsed[0][1] + ', аудитории ' + parsed.map((item) => item[2]).join(' и ');
      }
    }
    return rooms.join(' · ');
  }
  function slotSummary(slot) {
    const title = slotTitle(slot);
    const firstRoom = slot.entries.length ? formatEntryRooms(slot.entries[0]) : '';
    const extra = slot.entries.length > 1 ? ` · ещё ${slot.entries.length - 1} ${plural(slot.entries.length - 1, 'вариант', 'варианта', 'вариантов')}` : '';
    return [title, firstRoom, `в ${slot.start}${extra}`].filter(Boolean).join(' · ');
  }
  function pairOrdinal(pair) {
    const number = Number(pair);
    if (number === 1) return 'первой';
    return Number.isFinite(number) && number > 0 ? `${number}-й` : '';
  }
  function countPairs(lessons) {
    return groupLessonsBySlot(lessons).length;
  }
  function getStatus(date, lessons) {
    const slots = groupLessonsBySlot(lessons);
    if (dateKey(date) !== dateKey(startOfDay(new Date()))) {
      const count = slots.length;
      return {
        title: `${count} ${plural(count, 'пара', 'пары', 'пар')}`,
        subtitle: formatDate(date, { weekday: 'long', day: 'numeric', month: 'long' }),
        next: '',
      };
    }
    const parts = moscowDateParts();
    const current = Number(parts.hour) * 60 + Number(parts.minute);
    const activeIndex = slots.findIndex((slot) => current >= minutes(slot.start) && current < minutes(slot.end));
    const nextIndex = slots.findIndex((slot) => current < minutes(slot.start));
    const active = activeIndex >= 0 ? slots[activeIndex] : null;
    const next = nextIndex >= 0 ? slots[nextIndex] : null;
    if (active) {
      return {
        title: active.pair ? `Идёт ${Number(active.pair) === 1 ? 'первая' : `${active.pair}-я`} пара` : 'Идёт пара',
        subtitle: `${slotTitle(active)} · до ${active.end}`,
        next: next ? `Следующая: ${slotSummary(next)}` : 'После этой пары занятий больше нет',
      };
    }
    // University lunch window is fixed: 12:45–13:30.
    if (current >= 12 * 60 + 45 && current < 13 * 60 + 30) {
      return {
        title: 'Обед',
        subtitle: '12:45–13:30',
        next: next ? `Следующая: ${slotSummary(next)}` : 'На сегодня занятий больше нет',
      };
    }
    if (next) {
      const previous = slots.slice(0, nextIndex).at(-1);
      const nextPairLabel = next.pair ? `${pairOrdinal(next.pair)} пары` : 'следующей пары';
      if (!previous && Number(next.pair) > 1) {
        const lastPair = slots.reduce((last, slot) => Math.max(last, Number(slot.pair) || 0), 0);
        const total = slots.length;
        return {
          title: lastPair ? `Учёба до ${lastPair}-й пары` : 'Учебный день впереди',
          subtitle: 'Первая пара по расписанию не предусмотрена.',
          next: `Ближайшая: ${slotSummary(next)}`,
        };
      }
      return {
        title: previous ? 'Перемена' : 'До первой пары',
        subtitle: previous
          ? `До ${nextPairLabel} · начнётся в ${next.start}`
          : 'До начала занятий есть время.',
        next: `Следующая: ${slotSummary(next)}`,
      };
    }
    return {
      title: slots.length ? 'Занятия закончились' : 'Выходной',
      subtitle: slots.length ? 'На сегодня больше нет занятий.' : 'На этот день расписание пустое.',
      next: 'На сегодня занятий больше нет',
    };
  }
  function plural(value, one, few, many) {
    const abs = Math.abs(value) % 100;
    const last = abs % 10;
    if (abs > 10 && abs < 20) return many;
    if (last > 1 && last < 5) return few;
    if (last === 1) return one;
    return many;
  }
  function renderHero() {
    const lessons = getLessons(state.selectedDate, 'current');
    const today = dateKey(state.selectedDate) === dateKey(startOfDay(new Date()));
    const status = getStatus(state.selectedDate, lessons);
    $('#hero-label').textContent = today ? 'СЕГОДНЯ' : formatDate(state.selectedDate, { weekday: 'long' }).toUpperCase();
    $('#hero-title').textContent = status.title;
    $('#hero-subtitle').textContent = status.subtitle;
    $('#hero-next').textContent = status.next;
    $('#hero-next').hidden = !today || !status.next;
    $('.hero-card').classList.toggle('summary-only', !today);
    $('#hero-footer-text').textContent = !navigator.onLine ? 'Оффлайн · сохранённое расписание' : (state.schedule?.updated ? `Обновлено ${state.schedule.updated}` : 'Расписание загружено');
    $('#week-label').textContent = `${isOddWeek(state.selectedDate) ? 'нечётная' : 'чётная'} неделя`;
    $('#welcome-date').textContent = today ? 'ТВОЙ УЧЕБНЫЙ ДЕНЬ' : formatDate(state.selectedDate, { day: 'numeric', month: 'long' }).toUpperCase();
    $('#schedule-heading').textContent = formatDate(state.selectedDate, { day: 'numeric', month: 'long' });
    $('#today-button').textContent = 'Сегодня';
    $('#today-button').hidden = !today;
  }
  function renderDays() {
    const monday = weekStart(state.selectedDate);
    $('#day-strip').innerHTML = DAYS.map((day, index) => {
      const date = new Date(monday); date.setUTCDate(monday.getUTCDate() + index);
      const selected = dateKey(date) === dateKey(state.selectedDate);
      const today = dateKey(date) === dateKey(startOfDay(new Date()));
      return `<button class="day-chip${selected ? ' selected' : ''}${today ? ' today' : ''}" role="tab" aria-selected="${selected}" data-date="${dateKey(date)}"><span>${day.short}</span><span>${date.getUTCDate()}</span></button>`;
    }).join('');
    $('#day-strip').querySelectorAll('[data-date]').forEach((button) => button.addEventListener('click', () => {
      state.selectedDate = new Date(`${button.dataset.date}T12:00:00Z`);
      renderAll();
    }));
  }
  function lessonMarkup(slot, index, date) {
    const nowParts = moscowDateParts();
    const currentMinutes = Number(nowParts.hour) * 60 + Number(nowParts.minute);
    const today = dateKey(date) === dateKey(startOfDay(new Date()));
    const current = today && currentMinutes >= minutes(slot.start) && currentMinutes < minutes(slot.end);
    const commonTitle = slotTitle(slot);
    const hasCommonTitle = slot.entries.length > 1 && slot.entries.every((entry) => lessonBaseName(entry.name) === lessonBaseName(slot.entries[0].name));
    const entries = slot.entries.map((entry) => {
      const teacher = entry.teacher ? `<span class="meta-item"><svg class="meta-svg" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="7" r="3.5"/><path d="M5 21v-2a7 7 0 0 1 14 0v2z"/></svg>${escapeHtml(entry.teacher)}</span>` : '';
      const roomText = formatEntryRooms(entry);
      const room = roomText ? `<span class="meta-item"><svg class="meta-svg" viewBox="0 0 24 24" aria-hidden="true"><path d="M20 10c0 5-8 12-8 12S4 15 4 10a8 8 0 1 1 16 0Z"/><circle cx="12" cy="10" r="2.5"/></svg>${escapeHtml(roomText)}</span>` : '';
      const type = lessonType(entry.name);
      const label = hasCommonTitle ? type : (slot.entries.length > 1 ? compactLessonTitle(entry.name) : '');
      return `<div class="lesson-variant">${label ? `<span class="lesson-variant-type">${escapeHtml(label)}</span>` : ''}<div class="lesson-meta">${teacher}${room}</div></div>`;
    }).join('');
    return `<article class="lesson-card${current ? ' is-current' : ''}${slot.entries.length > 1 ? ' has-variants' : ''}" data-slot-index="${index}" style="animation-delay:${Math.min(index * 35, 175)}ms"><div class="lesson-time">${escapeHtml(slot.start)}<small>— ${escapeHtml(slot.end)}</small><small>${slot.pair ? `${escapeHtml(slot.pair)} пара` : ''}</small></div><div class="lesson-main">${current ? '<span class="current-tag">● Идёт сейчас</span>' : ''}<h3 class="lesson-title">${escapeHtml(commonTitle)}</h3><div class="lesson-variants">${entries}</div></div></article>`;
  }
  function renderSchedule() {
    const lessons = getLessons(state.selectedDate);
    const slots = groupLessonsBySlot(lessons);
    const list = $('#schedule-list');
    if (!state.schedule) {
      list.innerHTML = '<div class="empty-card"><span class="empty-illustration">✦</span><strong>Выбери группу</strong><p>После выбора появится расписание на день и неделю.</p></div>';
      $('#updated-note').textContent = '';
      $('#schedule-count').textContent = '';
      return;
    }
    if (!slots.length) {
      list.innerHTML = '<div class="empty-card"><span class="empty-illustration">☼</span><strong>Пар нет</strong><p>На этот день занятий не найдено. Проверь данные в ЕИОС.</p></div>';
    } else {
      list.innerHTML = slots.map((slot, index) => lessonMarkup(slot, index, state.selectedDate)).join('');
    }
    $('#updated-note').textContent = state.schedule.updated ? `Данные ЕИОС обновлены ${state.schedule.updated}` : 'Источник: расписание СПбГЛТУ';
    const isToday = dateKey(state.selectedDate) === dateKey(startOfDay(new Date()));
    let remainingSlots = slots;
    let firstPairLabel = '';
    if (isToday) {
      const nowParts = moscowDateParts();
      const nowMinutes = Number(nowParts.hour) * 60 + Number(nowParts.minute);
      const firstRemaining = slots.findIndex((slot) => nowMinutes < minutes(slot.end));
      remainingSlots = firstRemaining >= 0 ? slots.slice(firstRemaining) : [];
      if (firstRemaining >= 0 && slots[firstRemaining].pair) {
        const pairNumber = Number(slots[firstRemaining].pair);
        firstPairLabel = ` · ${pairNumber === 2 ? 'со' : 'с'} ${pairOrdinal(pairNumber)}`;
      }
    }
    $('#schedule-count').textContent = remainingSlots.length
      ? `${remainingSlots.length} ${plural(remainingSlots.length, 'пара', 'пары', 'пар')}${firstPairLabel}`
      : (isToday ? 'На сегодня всё' : 'Пар нет');
    list.querySelectorAll('[data-slot-index]').forEach((card) => {
      const open = () => openLessonDetails(slots[Number(card.dataset.slotIndex)], state.selectedDate);
      card.addEventListener('click', open);
      card.addEventListener('keydown', (event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); open(); } });
      card.setAttribute('tabindex', '0');
      card.setAttribute('role', 'button');
      card.setAttribute('aria-label', `Подробнее: ${slotTitle(slots[Number(card.dataset.slotIndex)])}`);
    });
  }
  function renderWeek() {
    const monday = weekStart(state.selectedDate);
    const sunday = new Date(monday); sunday.setUTCDate(monday.getUTCDate() + 6);
    const currentMonday = weekStart(startOfDay(new Date()));
    const isCurrentWeek = dateKey(monday) === dateKey(currentMonday);
    const parityLabel = isOddWeek(monday) ? 'Нечётная учебная неделя' : 'Чётная учебная неделя';
    $('#week-range').textContent = formatDate(monday, { day: 'numeric', month: 'long' }) + ' — ' + formatDate(sunday, { day: 'numeric', month: 'long', year: 'numeric' }) + ' · ' + parityLabel.toLowerCase();
    $('#week-current-label').textContent = isCurrentWeek ? 'Текущая неделя' : parityLabel;
    $('#week-list').innerHTML = DAYS.map((day, index) => {
      const date = new Date(monday); date.setUTCDate(monday.getUTCDate() + index);
      const slots = groupLessonsBySlot(getLessons(date, 'current'));
      const content = slots.length ? slots.map((slot, slotIndex) => {
        const meta = slot.entries.map((entry) => [entry.teacher, formatEntryRooms(entry)].filter(Boolean).join(' · ')).filter(Boolean).join(' / ');
        return '<button type="button" class="week-lesson" data-week-day="' + day.id + '" data-week-index="' + slotIndex + '"><div class="week-lesson-time">' + escapeHtml(slot.start) + '<br>' + escapeHtml(slot.end) + '</div><div><p class="week-lesson-title">' + escapeHtml(slotTitle(slot)) + '</p><span class="week-lesson-meta">' + (slot.entries.length > 1 ? slot.entries.length + ' варианта · ' : '') + escapeHtml(meta) + '</span></div></button>';
      }).join('') : '<p class="week-empty">Занятий нет</p>';
      const count = slots.length ? slots.length + ' ' + plural(slots.length, 'пара', 'пары', 'пар') : 'выходной';
      return '<article class="week-day"><div class="week-day-head"><span class="week-day-title">' + day.full + '<small>' + formatDate(date, { day: 'numeric', month: 'short' }) + '</small></span><span class="week-day-count">' + count + '</span></div>' + content + '</article>';
    }).join('');
    $('#week-list').querySelectorAll('[data-week-day]').forEach((button) => button.addEventListener('click', () => {
      const date = new Date(monday); date.setUTCDate(monday.getUTCDate() + Number(button.dataset.weekDay) - 1);
      const slots = groupLessonsBySlot(getLessons(date, 'current'));
      openLessonDetails(slots[Number(button.dataset.weekIndex)], date);
    }));
  }
  function renderAll() {
    updateGroupLabel();
    if (isWeekPage) {
      $('#today-view').classList.add('hidden');
      $('.hero-card').classList.add('hidden');
      $('#week-view').classList.remove('hidden');
      renderWeek();
      return;
    }
    renderHero();
    renderDays();
    renderSchedule();
  }
  function openGroupDialog() {
    renderGroupOptions('');
    const dialog = $('#group-dialog');
    if (!dialog.open) dialog.showModal();
    $('#group-search').value = '';
    $('#group-search').focus();
  }
  function renderGroupOptions(filter) {
    const normalized = filter.trim().toLocaleLowerCase('ru');
    const groups = state.groups.filter((group) => String(group.name).toLocaleLowerCase('ru').includes(normalized));
    const results = $('#group-results');
    if (!groups.length) { results.innerHTML = '<div class="empty-card"><strong>Не нашли такую группу</strong><p>Проверь обозначение или попробуй ввести часть названия.</p></div>'; return; }
    results.innerHTML = groups.slice(0, 100).map((group) => `<button class="group-option${String(group.id) === String(state.schedule?.id) ? ' selected' : ''}" data-id="${escapeHtml(group.id)}"><span>${escapeHtml(group.name)}</span><small>открыть</small></button>`).join('');
    results.querySelectorAll('[data-id]').forEach((button) => button.addEventListener('click', () => selectGroup(button.dataset.id)));
  }
  async function selectGroup(id) {
    const group = state.groups.find((item) => String(item.id) === String(id));
    if (!group) return;
    $('#group-results').innerHTML = '<p class="loading">Загружаем расписание…</p>';
    try {
      const response = await fetchDataFile(`${encodeURIComponent(group.id)}.json`);
      const schedule = await response.json();
      state.schedule = schedule;
      safeStorageSet(GROUP_KEY, String(group.id));
      $('#group-dialog').close();
      renderAll();
      await syncLessonReminders();
      showToast(`Группа ${group.name} сохранена`);
    } catch {
      showToast('Не получилось загрузить расписание. Попробуй ещё раз.');
      renderGroupOptions($('#group-search').value);
    }
  }
  async function loadData() {
    try {
      const response = await fetchDataFile('groups.json');
      state.groups = await response.json();
      const saved = safeStorageGet(GROUP_KEY) || safeStorageGet('groupId');
      const match = state.groups.find((group) => String(group.id) === String(saved));
      if (match) await selectGroup(match.id);
      else if (state.groups.length) { renderAll(); openGroupDialog(); }
      else throw new Error('no-groups');
    } catch {
      $('#hero-title').textContent = 'Расписание пока недоступно';
      $('#hero-subtitle').textContent = 'Проверь подключение и обнови экран';
      $('#hero-footer-text').textContent = 'Не удалось подключиться к источнику расписания';
      $('#schedule-list').innerHTML = '<div class="empty-card"><span class="empty-illustration">↻</span><strong>Не удалось загрузить данные</strong><p>Проверь подключение и нажми кнопку обновления. Если проблема повторится, попробуй позже.</p><button class="text-button" id="retry-load" type="button">Повторить загрузку</button></div>';
      $('#retry-load')?.addEventListener('click', () => loadData());
    }
  }

  let campusOnlineMap = null;
  let campusYandexMap = null;
  let campusMapMode = 'offline';
  let campusYandexApiPromise = null;
  const YANDEX_MAPS_API_KEY = '46171b6e-a2f6-4d81-8e7b-978252150eb7';
  const CAMPUS_PARK_BOUNDS = [[30.3304887, 59.9899431], [30.3542134, 59.9975551]];
  const campusMapMarkers = new Map();
  const campusSceneLabelMarkers = [];
  let campusMapLibraryPromise = null;
  function loadCampusMapLibrary() {
    if (window.maplibregl) return Promise.resolve(window.maplibregl);
    if (campusMapLibraryPromise) return campusMapLibraryPromise;
    campusMapLibraryPromise = new Promise((resolve, reject) => {
      if (!document.querySelector('link[data-campus-maplibre]')) {
        const link = document.createElement('link');
        link.rel = 'stylesheet';
        link.href = new URL('./vendor/maplibre-gl.css', APP_SCRIPT_URL).href;
        link.dataset.campusMaplibre = '1';
        document.head.appendChild(link);
      }
      const script = document.createElement('script');
      script.src = new URL('./vendor/maplibre-gl.js', APP_SCRIPT_URL).href;
      script.async = true;
      script.onload = () => window.maplibregl ? resolve(window.maplibregl) : reject(new Error('MapLibre не инициализировался'));
      script.onerror = () => reject(new Error('Не удалось загрузить библиотеку 3D-карты'));
      document.head.appendChild(script);
    });
    return campusMapLibraryPromise;
  }
  function setCampusMapMode(mode) {
    campusMapMode = mode;\n    if (mode !== 'online') setCampusSatelliteVisible(false);
    const online = mode === 'online' || mode === 'yandex';
    const panel = $('#campus-online-panel');
    const offlinePanel = $('#campus-offline-panel');
    if (!panel || !offlinePanel) return;
    panel.hidden = !online;
    offlinePanel.hidden = online;
    const osmMap = $('#campus-online-map');
    const yandexMap = $('#campus-yandex-map');
    if (osmMap) osmMap.hidden = mode !== 'online';
    if (yandexMap) yandexMap.hidden = mode !== 'yandex';
    $('#campus-mode-osm')?.setAttribute('aria-pressed', String(mode === 'online'));
    $('#campus-mode-yandex')?.setAttribute('aria-pressed', String(mode === 'yandex'));
    requestAnimationFrame(() => {
      if (mode === 'online') campusOnlineMap?.resize();
      if (mode === 'yandex') campusYandexMap?.container.fitToViewport();
    });
  }
  function setCampusSatelliteVisible(visible) {
    if (!campusOnlineMap?.getLayer('campus-satellite-raster')) return;
    campusOnlineMap.setLayoutProperty('campus-satellite-raster', 'visibility', visible ? 'visible' : 'none');
    const ground = campusOnlineMap.getLayer('campus-park-ground');
    if (ground) campusOnlineMap.setPaintProperty('campus-park-ground', 'fill-opacity', visible ? 0 : 1);
    const green = campusOnlineMap.getLayer('campus-park-green-areas');
    if (green) campusOnlineMap.setPaintProperty('campus-park-green-areas', 'fill-opacity', visible ? 0.12 : 1);
    const trees = campusOnlineMap.getLayer('campus-park-woodland-texture');
    if (trees) campusOnlineMap.setPaintProperty('campus-park-woodland-texture', 'fill-opacity', visible ? 0.16 : 0.96);
    const water = campusOnlineMap.getLayer('campus-park-water');
    if (water) campusOnlineMap.setPaintProperty('campus-park-water', 'fill-opacity', visible ? 0.28 : 0.96);
  }
  function setCampusBasemap(mode) {
    const satellite = mode === 'satellite';
    $('#campus-basemap-osm')?.setAttribute('aria-pressed', String(!satellite));
    $('#campus-basemap-satellite')?.setAttribute('aria-pressed', String(satellite));
    setCampusSatelliteVisible(satellite);
  }
  function loadCampusYandexApi() {
    if (window.ymaps?.Map) return new Promise((resolve) => window.ymaps.ready(() => resolve(window.ymaps)));
    if (campusYandexApiPromise) return campusYandexApiPromise;
    campusYandexApiPromise = new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = `https://api-maps.yandex.ru/2.1/?apikey=${encodeURIComponent(YANDEX_MAPS_API_KEY)}&lang=ru_RU`;
      script.async = true;
      script.onload = () => {
        if (!window.ymaps?.ready) { reject(new Error('Яндекс.Карты не инициализировались')); return; }
        window.ymaps.ready(() => resolve(window.ymaps));
      };
      script.onerror = () => reject(new Error('Не удалось загрузить API Яндекс.Карт'));
      document.head.appendChild(script);
    }).catch((error) => {
      campusYandexApiPromise = null;
      throw error;
    });
    return campusYandexApiPromise;
  }
  function fitCampusYandexMap() {
    if (!campusYandexMap) return;
    const bounds = [
      [CAMPUS_PARK_BOUNDS[0][1], CAMPUS_PARK_BOUNDS[0][0]],
      [CAMPUS_PARK_BOUNDS[1][1], CAMPUS_PARK_BOUNDS[1][0]]
    ];
    campusYandexMap.setBounds(bounds, { checkZoomRange: true, zoomMargin: 42 });
  }
  async function openCampusYandexMap() {
    const status = $('#campus-online-status');
    if (!state.campus) await loadCampusDirectory();
    if (!state.campus?.buildings?.length) {
      status.textContent = location.protocol === 'file:'
        ? 'Карта открыта как локальный файл: браузер блокирует загрузку данных. Открой приложение через GitHub Pages или локальный веб-сервер.'
        : 'Не удалось загрузить справочник корпусов. Проверь подключение и повтори попытку.';
      setCampusMapMode('offline');
      return;
    }
    setCampusMapMode('yandex');
    if (campusYandexMap) {
      requestAnimationFrame(() => { campusYandexMap.container.fitToViewport(); fitCampusYandexMap(); });
      return;
    }
    status.textContent = 'Подключаем спутниковую карту Яндекса…';
    try {
      const ymaps = await loadCampusYandexApi();
      if (!$('#map-dialog')?.open) return;
      campusYandexMap = new ymaps.Map('campus-yandex-map', {
        center: [59.9931, 30.3405],
        zoom: 15,
        type: 'yandex#satellite',
        controls: ['zoomControl', 'typeSelector']
      });
      let markerCount = 0;
      (state.campus?.buildings || []).forEach((building) => {
        if (!Number.isFinite(building.longitude) || !Number.isFinite(building.latitude)) return;
        const placemark = new ymaps.Placemark([building.latitude, building.longitude], {
          iconCaption: `Корпус ${building.number}`,
          balloonContentHeader: escapeHtml(building.name),
          balloonContentBody: escapeHtml(building.address || '')
        }, { preset: 'islands#darkGreenCircleDotIconWithCaption' });
        placemark.events.add('click', () => selectCampusBuilding(building.number, { openPopup: false }));
        campusYandexMap.geoObjects.add(placemark);
        markerCount += 1;
      });
      fitCampusYandexMap();
      status.textContent = markerCount === 4
        ? 'Спутниковая карта Яндекса загружена. На ней отмечены все четыре корпуса.'
        : `Карта Яндекса открылась, но удалось отметить только ${markerCount} из 4 корпусов. Проверь данные справочника.`;
    } catch (error) {
      console.warn('Campus Yandex map failed:', error);
      status.textContent = `${error.message}. Проверь ключ и ограничение HTTP Referer.`;
    }
  }
  function fitCampusMapToBuildings(duration = 500) {
    if (!campusOnlineMap || !window.maplibregl) return;
    const element = $('#campus-online-map');
    const buildings = (state.campus?.buildings || []).filter((building) =>
      Number.isFinite(building.longitude) && Number.isFinite(building.latitude));
    if (!element || !buildings.length) return;
    const bounds = new window.maplibregl.LngLatBounds();
    buildings.forEach((building) => bounds.extend([building.longitude, building.latitude]));
    requestAnimationFrame(() => {
      if (!campusOnlineMap || !element.clientWidth || !element.clientHeight) return;
      campusOnlineMap.resize();
      const keepAllMarkersVisible = () => {
        if (!campusOnlineMap || !$('#map-dialog')?.open) return;
        const { width, height } = campusOnlineMap.getContainer().getBoundingClientRect();
        let zoom = campusOnlineMap.getZoom();
        for (let attempt = 0; attempt < 10; attempt += 1) {
          const allVisible = buildings.every((building) => {
            const point = campusOnlineMap.project([building.longitude, building.latitude]);
            return point.x >= 38 && point.x <= width - 38 && point.y >= 38 && point.y <= height - 38;
          });
          if (allVisible || zoom <= 11) break;
          zoom = Math.max(11, zoom - 0.25);
          campusOnlineMap.setZoom(zoom);
        }
      };
      if (duration > 0) campusOnlineMap.once('moveend', keepAllMarkersVisible);
      campusOnlineMap.fitBounds(bounds, {
        padding: { top: 76, right: 68, bottom: 76, left: 68 },
        maxZoom: 15,
        pitch: 35,
        duration
      });
      if (!duration) requestAnimationFrame(() => requestAnimationFrame(keepAllMarkersVisible));
    });
  }
  function fitCampusMapToPark(duration = 450) {
    if (!campusOnlineMap || !window.maplibregl) return;
    requestAnimationFrame(() => {
      if (!campusOnlineMap || !$('#campus-online-map')?.clientWidth) return;
      campusOnlineMap.resize();
      campusOnlineMap.fitBounds(CAMPUS_PARK_BOUNDS, {
        padding: { top: 22, right: 22, bottom: 22, left: 22 },
        maxZoom: 14.2,
        pitch: 58,
        bearing: -18,
        duration
      });
    });
  }
  function createCampusWoodlandTexture() {
    const size = 96;
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Не удалось создать текстуру леса');
    let seed = 5622070;
    const random = () => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed / 4294967296;
    };
    const crowns = Array.from({ length: 112 }, () => ({
      x: random() * size,
      y: random() * size,
      radius: 1.5 + random() * 2.4,
      tone: random()
    }));
    for (const crown of crowns) {
      for (const xOffset of [-size, 0, size]) {
        for (const yOffset of [-size, 0, size]) {
          const x = crown.x + xOffset;
          const y = crown.y + yOffset;
          const radius = crown.radius;
          context.fillStyle = 'rgba(35, 66, 42, 0.24)';
          context.beginPath();
          context.ellipse(x + radius * 0.35, y + radius * 0.55, radius * 1.18, radius * 0.88, 0, 0, Math.PI * 2);
          context.fill();
          const gradient = context.createRadialGradient(x - radius * 0.28, y - radius * 0.36, radius * 0.12, x, y, radius * 1.3);
          gradient.addColorStop(0, crown.tone > 0.74 ? '#a8c477' : '#86ad68');
          gradient.addColorStop(0.34, crown.tone > 0.48 ? '#6f9b5d' : '#5c8c54');
          gradient.addColorStop(0.78, '#477749');
          gradient.addColorStop(1, 'rgba(48, 89, 54, 0.88)');
          context.fillStyle = gradient;
          context.beginPath();
          context.ellipse(x, y, radius, radius * (0.76 + crown.tone * 0.22), crown.tone * 0.4, 0, Math.PI * 2);
          context.fill();
          context.fillStyle = 'rgba(192, 213, 139, 0.28)';
          context.beginPath();
          context.arc(x - radius * 0.32, y - radius * 0.3, Math.max(0.5, radius * 0.24), 0, Math.PI * 2);
          context.fill();
        }
      }
    }
    return context.getImageData(0, 0, size, size);
  }
  async function addCampusParkLayers(map) {
    const dataUrl = new URL('../data/campus-park-scene.json', APP_SCRIPT_URL).href;
    const response = await fetch(dataUrl, { cache: 'force-cache' });
    if (!response.ok) throw new Error(`Геометрия парка: HTTP ${response.status}`);
    const scene = await response.json();
    if (campusOnlineMap !== map) return null;
    if (!map.hasImage('campus-woodland-texture')) {
      map.addImage('campus-woodland-texture', createCampusWoodlandTexture(), { pixelRatio: 1 });
    }
    map.addSource('campus-park-scene', { type: 'geojson', data: scene });
    // Aerial raster is opt-in: MapLibre's gesture controls stay attached to the map,
    // while imagery is served as XYZ tiles (never copied into the repository).
    map.addSource('campus-satellite', {
      type: 'raster',
      tiles: ['https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}'],
      tileSize: 256,
      attribution: 'Tiles © Esri — Source: Esri, Maxar, Earthstar Geographics, and the GIS User Community',
      maxzoom: 19
    });
    scene.features.filter((feature) => feature.properties?.kind === 'water' && feature.properties?.name).forEach((feature) => {
      const coordinates = feature.geometry.type === 'Polygon'
        ? feature.geometry.coordinates[0]
        : feature.geometry.coordinates[0]?.[0];
      if (!coordinates?.length) return;
      const center = coordinates.reduce((sum, point) => [sum[0] + point[0] / coordinates.length, sum[1] + point[1] / coordinates.length], [0, 0]);
      const label = document.createElement('span');
      label.className = 'campus-map-area-label';
      label.textContent = feature.properties.name;
      campusSceneLabelMarkers.push(new window.maplibregl.Marker({ element: label, anchor: 'center' }).setLngLat(center).addTo(map));
    });
    map.addLayer({
      id: 'campus-park-ground', type: 'fill', source: 'campus-park-scene',
      filter: ['==', ['get', 'kind'], 'park-boundary'],
      paint: { 'fill-color': '#b8d49b', 'fill-opacity': 1 }
    });
    map.addLayer({
      id: 'campus-park-green-areas', type: 'fill', source: 'campus-park-scene',
      filter: ['==', ['get', 'kind'], 'green-area'],
      paint: {
        'fill-color': ['match', ['get', 'areaType'], 'wood', '#77996c', '#9fc782'],
        'fill-opacity': 1,
        'fill-outline-color': ['match', ['get', 'areaType'], 'wood', '#55764e', '#83ae70']
      }
    });
    map.addLayer({
      id: 'campus-park-woodland-texture', type: 'fill', source: 'campus-park-scene',
      filter: ['all', ['==', ['get', 'kind'], 'green-area'], ['==', ['get', 'areaType'], 'wood']],
      paint: { 'fill-pattern': 'campus-woodland-texture', 'fill-opacity': 0.96 }
    });
    map.addLayer({
      id: 'campus-park-water', type: 'fill', source: 'campus-park-scene',
      filter: ['==', ['get', 'kind'], 'water'],
      paint: { 'fill-color': '#73b9d4', 'fill-opacity': 0.96, 'fill-outline-color': '#4a96b0' }
    });
    map.addLayer({
      id: 'campus-park-boundary', type: 'line', source: 'campus-park-scene',
      filter: ['==', ['get', 'kind'], 'park-boundary'],
      paint: { 'line-color': '#54754c', 'line-width': 2.5, 'line-opacity': 0.9 }
    });
    map.addLayer({
      id: 'campus-park-path-casing', type: 'line', source: 'campus-park-scene',
      filter: ['==', ['get', 'kind'], 'path'],
      paint: { 'line-color': '#aaad89', 'line-width': ['interpolate', ['linear'], ['zoom'], 12, 1.2, 16, 4.5], 'line-opacity': 0.72 }
    });
    map.addLayer({
      id: 'campus-park-paths', type: 'line', source: 'campus-park-scene',
      filter: ['==', ['get', 'kind'], 'path'],
      paint: { 'line-color': '#f5edcf', 'line-width': ['interpolate', ['linear'], ['zoom'], 12, 0.65, 16, 2.5], 'line-opacity': 0.98 }
    });
    map.addLayer({
      id: 'campus-park-building-footprints', type: 'fill', source: 'campus-park-scene', minzoom: 12,
      filter: ['all', ['==', ['get', 'kind'], 'building'], ['==', ['get', 'heightKnown'], false]],
      paint: {
        'fill-color': '#b9b8b3',
        'fill-opacity': 0.96,
        'fill-outline-color': '#807f79'
      }
    });
    map.addLayer({
      id: 'campus-park-buildings', type: 'fill-extrusion', source: 'campus-park-scene', minzoom: 12,
      filter: ['all', ['==', ['get', 'kind'], 'building'], ['==', ['get', 'heightKnown'], true]],
      paint: {
        'fill-extrusion-color': ['coalesce', ['get', 'styleColor'], '#c7c4bc'],
        'fill-extrusion-height': ['get', 'heightMeters'],
        'fill-extrusion-base': 0,
        'fill-extrusion-opacity': 1,
        'fill-extrusion-vertical-gradient': true
      }
    });
    map.addLayer({
      id: 'campus-selected-building-footprint-highlight', type: 'fill', source: 'campus-park-scene', minzoom: 13,
      filter: ['all', ['==', ['get', 'kind'], 'building'], ['==', ['get', 'heightKnown'], false], ['==', ['get', 'osmId'], '']],
      paint: { 'fill-color': '#d8f08d', 'fill-opacity': 0.95, 'fill-outline-color': '#496f40' }
    });
    map.addLayer({
      id: 'campus-selected-building-highlight', type: 'fill-extrusion', source: 'campus-park-scene', minzoom: 13,
      filter: ['==', ['get', 'osmId'], ''],
      paint: {
        'fill-extrusion-color': '#d8f08d',
        'fill-extrusion-height': ['+', ['get', 'heightMeters'], 0.28],
        'fill-extrusion-base': ['max', 0, ['-', ['get', 'heightMeters'], 0.12]],
        'fill-extrusion-opacity': 0.96,
        'fill-extrusion-vertical-gradient': false
      }
    });
    map.addLayer({
      id: 'campus-park-tree-trunks', type: 'fill-extrusion', source: 'campus-park-scene', minzoom: 13,
      filter: ['==', ['get', 'kind'], 'tree-trunk'],
      paint: { 'fill-extrusion-color': '#79563a', 'fill-extrusion-height': ['get', 'heightMeters'], 'fill-extrusion-base': 0, 'fill-extrusion-opacity': 1 }
    });
    map.addLayer({
      id: 'campus-park-tree-crowns', type: 'fill-extrusion', source: 'campus-park-scene', minzoom: 13,
      filter: ['==', ['get', 'kind'], 'tree-canopy'],
      paint: {
        'fill-extrusion-color': ['get', 'foliageColor'],
        'fill-extrusion-height': ['get', 'heightMeters'],
        'fill-extrusion-base': ['get', 'canopyBaseMeters'],
        'fill-extrusion-opacity': 1,
        'fill-extrusion-vertical-gradient': true
      }
    });
    map.addLayer({
      id: 'campus-park-amenities', type: 'circle', source: 'campus-park-scene',
      filter: ['==', ['get', 'kind'], 'amenity'],
      paint: { 'circle-radius': 3, 'circle-color': '#fff7df', 'circle-stroke-color': '#6d8160', 'circle-stroke-width': 1 }
    });
    return scene.metadata;
  }
  function appendCampusPopupLink(container, href, label) {
    if (!href) return;
    let url;
    try { url = new URL(href, location.href); } catch { return; }
    if (!['https:', 'http:'].includes(url.protocol)) return;
    const link = document.createElement('a');
    link.href = url.href;
    link.target = '_blank';
    link.rel = 'noopener noreferrer';
    link.textContent = label;
    container.append(link);
  }
  function createCampusBuildingPopup(properties, building = null) {
    const root = document.createElement('article');
    root.className = 'campus-map-building-popup';
    const heritage = building?.heritage;
    const photo = heritage?.photo;
    if (photo?.src) {
      const figure = document.createElement('figure');
      figure.className = 'campus-map-building-photo';
      const image = document.createElement('img');
      image.src = new URL(photo.src, APP_SCRIPT_URL).href;
      image.alt = photo.alt || building.name;
      image.loading = 'lazy';
      image.decoding = 'async';
      image.addEventListener('error', () => figure.remove(), { once: true });
      figure.append(image);
      const credit = document.createElement('figcaption');
      credit.append(document.createTextNode(`${photo.credit || 'Фото Wikimedia Commons'} · `));
      appendCampusPopupLink(credit, photo.sourceUrl, 'источник');
      credit.append(document.createTextNode(' · '));
      appendCampusPopupLink(credit, photo.licenseUrl, photo.license || 'лицензия');
      figure.append(credit);
      root.append(figure);
    }

    const heading = document.createElement('strong');
    heading.className = 'campus-map-building-title';
    heading.textContent = building?.name || properties.name || `Здание парка · OSM ${properties.osmId || ''}`;
    root.append(heading);
    if (building?.address) {
      const address = document.createElement('div');
      address.className = 'campus-map-building-address';
      address.textContent = building.address;
      root.append(address);
    }
    if (building?.description) {
      const description = document.createElement('p');
      description.className = 'campus-map-building-description';
      description.textContent = building.description;
      root.append(description);
    }

    const facts = [];
    const typeLabels = { university: 'учебное здание', school: 'учебное здание', apartments: 'жилой дом', service: 'служебное здание', bunker: 'убежище', ruins: 'руины' };
    const styleLabels = { art_nouveau: 'модерн', empire: 'ампир', classicism: 'классицизм', baroque: 'барокко' };
    const claddingLabels = { plaster: 'штукатурка', brick: 'кирпич', wood: 'дерево', stone: 'камень', concrete: 'бетон', glass: 'стекло' };
    const roofLabels = { hipped: 'вальмовая крыша', gabled: 'двускатная крыша', flat: 'плоская крыша', mansard: 'мансардная крыша' };
    const levels = Number(building?.osm?.levels ?? properties.levels);
    const height = Number(building?.osm?.heightMeters ?? properties.heightMeters);
    const heightSource = String(properties.heightSource || '');
    if (Number.isFinite(levels) && levels > 0) facts.push(`${levels} эт.`);
    if (heightSource.startsWith('unknown') || heightSource.startsWith('visual default')) {
      facts.push('высота не указана в источнике');
    } else if (Number.isFinite(height) && height > 0) {
      facts.push(heightSource.includes('estimate') ? `около ${height} м · оценка по этажности` : `${height} м по данным OSM`);
    }
    const cladding = building?.osm?.cladding || properties.cladding;
    const roofShape = building?.osm?.roofShape || properties.roofShape;
    const architecture = properties.architecturalStyle;
    if (claddingLabels[cladding]) facts.push(`фасад: ${claddingLabels[cladding]}`);
    if (roofLabels[roofShape]) facts.push(roofLabels[roofShape]);
    if (styleLabels[architecture]) facts.push(`архитектура: ${styleLabels[architecture]}`);
    if (typeLabels[properties.osmBuilding]) facts.push(typeLabels[properties.osmBuilding]);
    if (facts.length) {
      const list = document.createElement('p');
      list.className = 'campus-map-building-facts';
      list.textContent = facts.join(' · ');
      root.append(list);
    }

    if (heritage) {
      const history = document.createElement('div');
      history.className = 'campus-map-building-history';
      const heritageLine = document.createElement('strong');
      heritageLine.textContent = `Объект культурного наследия · № ${heritage.heritageId}`;
      const historyFacts = document.createElement('p');
      historyFacts.textContent = `${heritage.built}. ${heritage.style}. Архитекторы: ${heritage.architects}.`;
      history.append(heritageLine, historyFacts);
      const sources = document.createElement('div');
      sources.className = 'campus-map-building-links';
      appendCampusPopupLink(sources, heritage.heritageSourceUrl, 'История и охранный статус ↗');
      appendCampusPopupLink(sources, heritage.historySourceUrl, 'Историческая карта LAYRS ↗');
      history.append(sources);
      root.append(history);
    }

    const links = document.createElement('div');
    links.className = 'campus-map-building-links';
    appendCampusPopupLink(links, properties.sourceUrl || building?.osm?.url, 'Контур и данные OpenStreetMap ↗');
    if (String(building?.number) === '1') {
      appendCampusPopupLink(links, 'https://yandex.ru/maps/org/spbgltu_im_s_m_kirova_aspirantura/98448509917/?l=stv%2Csta&ll=30.343612%2C59.992531&panorama%5Bdirection%5D=48.801822%2C17.418881&panorama%5Bfull%5D=true&panorama%5Bpoint%5D=30.343210%2C59.992557&panorama%5Bspan%5D=117.625646%2C60.000000&z=18.78', 'Фасад в панораме Яндекса ↗');
    }
    if (links.childNodes.length) root.append(links);
    return root;
  }
  function installCampusBuildingInteractions(map, maplibregl) {
    const layerIds = ['campus-park-buildings', 'campus-park-building-footprints'].filter((id) => map.getLayer(id));
    if (!layerIds.length) return;
    const showPointer = () => { map.getCanvas().style.cursor = 'pointer'; };
    const clearPointer = () => { map.getCanvas().style.cursor = ''; };
    layerIds.forEach((layerId) => {
      map.on('mouseenter', layerId, showPointer);
      map.on('mouseleave', layerId, clearPointer);
      map.on('click', layerId, (event) => {
        const properties = event.features?.[0]?.properties;
        if (!properties) return;
        const building = state.campus?.buildings?.find((item) => String(item.osm?.id) === String(properties.osmId));
        document.querySelectorAll('.campus-building-card.selected').forEach((card) => card.classList.remove('selected'));
        const osmId = String(properties.osmId);
        if (building) selectCampusBuilding(building.number, { openPopup: false });
        if (map.getLayer('campus-selected-building-highlight')) {
          map.setFilter('campus-selected-building-highlight', ['==', ['get', 'osmId'], osmId]);
        }
        if (map.getLayer('campus-selected-building-footprint-highlight')) {
          map.setFilter('campus-selected-building-footprint-highlight', ['==', ['get', 'osmId'], osmId]);
        }
        new maplibregl.Popup({ closeButton: true, closeOnClick: true, offset: 12, maxWidth: '320px' })
          .setLngLat(event.lngLat)
          .setDOMContent(createCampusBuildingPopup(properties, building))
          .addTo(map);
      });
    });
  }
  async function openCampusOnlineMap() {
    const panel = $('#campus-online-panel');
    const status = $('#campus-online-status');
    if (!panel) return;
    if (!state.campus) await loadCampusDirectory();
    setCampusMapMode('online');
    if (campusOnlineMap) { fitCampusMapToPark(); return; }
    status.textContent = 'Загружаем автономную 3D-карту парка…';
    try {
      const maplibregl = await loadCampusMapLibrary();
      campusOnlineMap = new maplibregl.Map({
        container: 'campus-online-map',
        style: {
          version: 8,
          sources: {},
          layers: [{ id: 'campus-background', type: 'background', paint: { 'background-color': '#d9d5bf' } }]
        },
        center: [30.34235, 59.99375],
        zoom: 13.8,
        maxBounds: CAMPUS_PARK_BOUNDS,
        pitch: 58,
        bearing: -18,
        maxPitch: 68,
        antialias: true,
        attributionControl: true
      });
      campusOnlineMap.addControl(new maplibregl.NavigationControl({ visualizePitch: true }), 'top-right');
      // Restrict native touch handling to the map canvas. The dialog and directory
      // retain their own scrolling surface outside the map container.
      campusOnlineMap.dragRotate.enable({ around: 'center' });
      if (window.matchMedia('(pointer: coarse)').matches) {
        campusOnlineMap.dragPan.enable();
        campusOnlineMap.touchZoomRotate.enable();
      }
      let styleLoaded = false;
      campusOnlineMap.on('load', async () => {
        styleLoaded = true;
        campusMapMarkers.clear();
        const buildings = state.campus?.buildings || [];
        let markerCount = 0;
        buildings.forEach((building) => {
          if (!Number.isFinite(building.longitude) || !Number.isFinite(building.latitude)) return;
          const marker = document.createElement('button');
          marker.type = 'button';
          marker.className = 'campus-map-marker';
          marker.setAttribute('aria-label', `Выбрать корпус ${building.number}`);
          const markerLabel = document.createElement('span');
          markerLabel.textContent = String(building.number);
          marker.append(markerLabel);
          const markerProperties = {
            osmId: String(building.osm?.id || ''),
            osmBuilding: building.osm?.building || '',
            levels: building.osm?.levels || '',
            heightMeters: building.osm?.heightMeters || '',
            cladding: building.osm?.cladding || '',
            roofShape: building.osm?.roofShape || '',
            sourceUrl: building.osm?.url || ''
          };
          const popup = createCampusBuildingPopup(markerProperties, building);
          const mapMarker = new maplibregl.Marker({ element: marker, anchor: 'bottom' })
            .setLngLat([building.longitude, building.latitude])
            .setPopup(new maplibregl.Popup({ offset: 22, maxWidth: '320px' }).setDOMContent(popup))
            .addTo(campusOnlineMap);
          campusMapMarkers.set(String(building.number), mapMarker);
          marker.addEventListener('click', () => selectCampusBuilding(building.number, { openPopup: false }));
          markerCount += 1;
        });
        try {
          const metadata = await addCampusParkLayers(campusOnlineMap);
          if (metadata && campusOnlineMap && $('#map-dialog')?.open) {
            status.textContent = `Автономная 3D-карта парка загружена: ${metadata.counts.building || 0} зданий, ${metadata.counts.path || 0} дорожек, водоёмы и зелёные зоны. Геометрия сохранена на устройстве.`;
          }
          if (metadata && campusOnlineMap) installCampusBuildingInteractions(campusOnlineMap, maplibregl);
        } catch (error) {
          console.warn('Campus park scene failed:', error);
          status.textContent = `${error.message}. Справочник корпусов остаётся доступен.`;
        }
        const fitButton = $('#campus-fit-buildings');
        if (fitButton) fitButton.disabled = markerCount === 0;
        fitCampusMapToPark(0);
      });
      campusOnlineMap.on('error', (event) => {
        if (!styleLoaded && campusOnlineMap && event?.error) {
          console.warn('Campus map failed before style load:', event.error);
          status.textContent = 'Онлайн-карта сейчас недоступна. Справочник корпусов работает офлайн.';
          const failedMap = campusOnlineMap;
          campusOnlineMap = null;
          campusMapMarkers.clear();
          failedMap.remove();
          const fallbackStatus = $('#campus-offline-status');
          if (fallbackStatus) fallbackStatus.textContent = 'Онлайн-карта сейчас недоступна. Проверь подключение и повтори попытку; справочник остаётся доступен ниже.';
          setCampusMapMode('offline');
        } else if (styleLoaded && event?.error) {
          console.warn('Campus map layer failed to load:', event.error);
          status.textContent = 'Часть слоёв карты не загрузилась. Попробуй обновить карту или проверь подключение.';
        }
      });
    } catch (error) {
      status.textContent = `${error.message}. Справочник корпусов работает офлайн.`;
      campusMapLibraryPromise = null;
      const failedMap = campusOnlineMap;
      campusOnlineMap = null;
      campusMapMarkers.clear();
      failedMap?.remove();
      const fallbackStatus = $('#campus-offline-status');
      if (fallbackStatus) fallbackStatus.textContent = 'Не удалось загрузить онлайн-карту. Проверь подключение и повтори попытку; список корпусов остаётся доступен ниже.';
      setCampusMapMode('offline');
    }
  }
  function selectCampusBuilding(number, { openPopup = true } = {}) {
    const selected = String(number);
    document.querySelectorAll('.campus-building-card').forEach((node) => {
      node.classList.toggle('selected', node.dataset.building === selected);
    });
    const building = state.campus?.buildings?.find((item) => String(item.number) === selected);
    const mapMarker = campusMapMarkers.get(selected);
    if (building && campusMapMode === 'yandex' && campusYandexMap) {
      campusYandexMap.setCenter([building.latitude, building.longitude], Math.max(campusYandexMap.getZoom(), 17), { duration: 350 });
    } else if (building && campusOnlineMap && mapMarker) {
      campusOnlineMap.flyTo({ center: [building.longitude, building.latitude], zoom: Math.max(campusOnlineMap.getZoom(), 16), duration: 450 });
      if (building.osm?.id != null) {
        const osmId = String(building.osm.id);
        if (campusOnlineMap.getLayer('campus-selected-building-highlight')) {
          campusOnlineMap.setFilter('campus-selected-building-highlight', ['==', ['get', 'osmId'], osmId]);
        }
        if (campusOnlineMap.getLayer('campus-selected-building-footprint-highlight')) {
          campusOnlineMap.setFilter('campus-selected-building-footprint-highlight', ['==', ['get', 'osmId'], osmId]);
        }
      }
      if (openPopup) mapMarker.togglePopup();
    }
    const card = document.querySelector(`.campus-building-card[data-building="${selected}"]`);
    if (card && $('#campus-search')?.value) card.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  }
  function renderCampusDirectory(query = '') {
    const target = $('#campus-buildings');
    if (!target) return;
    const normalized = String(query).trim().toLocaleLowerCase('ru');
    if (!state.campus?.buildings?.length) {
      target.innerHTML = '<p class="campus-empty">Справочник корпусов пока не загружен.</p>';
      return;
    }
    const cards = state.campus.buildings.map((building) => {
      const units = (building.units || []).filter((unit) => {
        const haystack = `${building.name} ${building.description || ''} ${unit.name} ${unit.room || ''}`.toLocaleLowerCase('ru');
        return !normalized || haystack.includes(normalized);
      });
      if (normalized && !units.length && !building.name.toLocaleLowerCase('ru').includes(normalized)) return '';
      return `<article class="campus-building-card" data-building="${escapeHtml(building.number)}" tabindex="0" role="button" aria-label="Показать корпус ${escapeHtml(building.number)}">
        <div class="campus-building-head"><span class="campus-building-number">${escapeHtml(building.number)}</span><div><h3>${escapeHtml(building.name)}</h3>${building.description ? `<p>${escapeHtml(building.description)}</p>` : ''}</div></div>
        <ul>${units.length ? units.map((unit) => `<li><span>${escapeHtml(unit.name)}</span><strong>${escapeHtml(unit.room || 'без номера')}</strong></li>`).join('') : '<li class="campus-no-match">Нет совпадений по подразделениям</li>'}</ul>
      </article>`;
    }).join('');
    target.innerHTML = cards || '<p class="campus-empty">Ничего не найдено. Попробуй номер кабинета или название подразделения.</p>';
    target.querySelectorAll('.campus-building-card').forEach((card) => {
      const choose = () => selectCampusBuilding(card.dataset.building);
      card.addEventListener('click', choose);
      card.addEventListener('keydown', (event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); choose(); } });
    });
  }
  async function loadCampusDirectory() {
    if (state.campus) {
      renderCampusDirectory($('#campus-search')?.value || '');
      return;
    }
    const target = $('#campus-buildings');
    if (target) target.innerHTML = '<p class="campus-empty">Загружаем справочник…</p>';
    try {
      const response = await fetchDataFile('campus.json');
      state.campus = await response.json();
      renderCampusDirectory($('#campus-search')?.value || '');
    } catch {
      if (target) target.innerHTML = '<p class="campus-empty">Справочник недоступен. Открой приложение с интернетом хотя бы один раз, чтобы сохранить его на устройстве.</p>';
    }
  }

  function switchView(view) {
    if (view !== 'today') return;
    location.href = './index.html';
  }
  function openWeek(date) {
    const url = new URL('./week.html', location.href);
    url.searchParams.set('date', dateKey(date));
    location.href = url.href;
  }

  function closeQuickMenu() {
    $('#quick-menu').classList.remove('open');
    $('#quick-main').classList.remove('active');
    $('#quick-main').setAttribute('aria-expanded', 'false');
  }
  function changeCalendarMonth(delta) {
    const candidate = new Date(state.calendarYear, state.calendarMonth + delta, 1);
    if (candidate < new Date(2026, 8, 1) || candidate > new Date(2027, 6, 31)) return;
    state.calendarMonth = candidate.getMonth();
    state.calendarYear = candidate.getFullYear();
    renderCalendar();
  }
  function renderCalendar() {
    const months = ['Январь','Февраль','Март','Апрель','Май','Июнь','Июль','Август','Сентябрь','Октябрь','Ноябрь','Декабрь'];
    $('#calendar-month-name').textContent = `${months[state.calendarMonth]} ${state.calendarYear}`;
    const grid = $('#calendar-grid');
    const headers = ['Пн','Вт','Ср','Чт','Пт','Сб','Вс'].map((day) => `<span class="calendar-day-head">${day}</span>`).join('');
    const first = new Date(state.calendarYear, state.calendarMonth, 1);
    const offset = (first.getDay() + 6) % 7;
    const total = new Date(state.calendarYear, state.calendarMonth + 1, 0).getDate();
    const today = dateKey(startOfDay(new Date()));
    let cells = '';
    for (let i = 0; i < offset; i++) cells += '<span class="calendar-day empty"></span>';
    for (let d = 1; d <= total; d++) {
      const date = new Date(Date.UTC(state.calendarYear, state.calendarMonth, d, 12));
      const key = dateKey(date);
      const parity = isOddWeek(date) ? 'odd-week' : 'even-week';
      cells += `<button type="button" class="calendar-day ${parity} ${key === today ? 'today' : ''}" data-calendar-date="${key}">${d}</button>`;
    }
    grid.innerHTML = headers + cells;
    grid.querySelectorAll('[data-calendar-date]').forEach((button) => button.addEventListener('click', () => {
      const date = new Date(`${button.dataset.calendarDate}T12:00:00Z`);
      const lessons = getLessons(date, 'current');
      $('#calendar-details').innerHTML = '<strong>' + escapeHtml(formatDate(date, { weekday: 'long', day: 'numeric', month: 'long' })) + '</strong><div class="calendar-lesson-list">' + (lessons.length ? lessons.map((lesson) => '<p><b>' + escapeHtml(lesson.start) + '–' + escapeHtml(lesson.end) + '</b> ' + escapeHtml(lesson.name) + '<small>' + escapeHtml(parseRoom(lesson.room)) + '</small></p>').join('') : '<p>Пар нет</p>') + '</div><div class="calendar-actions"><button type="button" class="text-button" id="calendar-open-day">Открыть день</button><button type="button" class="text-button" id="calendar-open-week">Открыть неделю</button></div>';
      $('#calendar-open-day').addEventListener('click', () => {
        state.selectedDate = date;
        $('#calendar-dialog').close();
        renderAll();
      });
      $('#calendar-open-week').addEventListener('click', () => openWeek(date));
    }));
  }
  async function loadWeather() {
    const box = $('#weather-content');
    box.textContent = 'Загружаем погоду…';
    try {
      const response = await fetch('https://api.open-meteo.com/v1/forecast?latitude=59.9386&longitude=30.3141&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m&daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=Europe%2FMoscow');
      if (!response.ok) throw new Error('weather');
      const data = await response.json();
      const labels = {0:'Ясно',1:'Преимущественно ясно',2:'Переменная облачность',3:'Пасмурно',45:'Туман',48:'Изморозь',51:'Морось',53:'Морось',55:'Сильная морось',61:'Небольшой дождь',63:'Дождь',65:'Сильный дождь',71:'Снег',73:'Снегопад',75:'Сильный снегопад',80:'Ливень',81:'Ливень',82:'Сильный ливень',95:'Гроза'};
      const current = data.current, daily = data.daily;
      box.innerHTML = `<div class="weather-current"><strong>${Math.round(current.temperature_2m)}°</strong><span>${labels[current.weather_code] || 'Погодные условия'}</span><small>Ощущается как ${Math.round(current.apparent_temperature)}° · ветер ${Math.round(current.wind_speed_10m)} км/ч · влажность ${current.relative_humidity_2m}%</small></div><div class="weather-days">${daily.time.slice(0, 5).map((date, i) => `<div><b>${new Date(date + 'T12:00:00').toLocaleDateString('ru-RU', { weekday: 'short' })}</b><span>${labels[daily.weather_code[i]] || '—'}</span><strong>${Math.round(daily.temperature_2m_max[i])}° / ${Math.round(daily.temperature_2m_min[i])}°</strong></div>`).join('')}</div>`;
    } catch {
      box.textContent = 'Не удалось загрузить погоду. Проверь подключение к интернету.';
    }
  }
  function openLessonDetails(slot, date) {
    if (!slot) return;
    $('#detail-date').textContent = formatDate(date, { weekday: 'long', day: 'numeric', month: 'long' }).toUpperCase();
    $('#detail-name').textContent = slotTitle(slot);
    $('#detail-time').textContent = `${slot.start} — ${slot.end}${slot.pair ? ` · ${slot.pair} пара` : ''}`;
    const teacherLines = slot.entries.map((entry) => {
      const teacherNames = String(entry.teacher || '').split(',').map((name) => name.trim()).filter(Boolean);
      const teacherLinks = teacherNames.map((short) => {
        const data = teachersDB[short];
        return data?.url ? `<a class="teacher-profile" href="${escapeHtml(data.url)}" target="_blank" rel="noopener">${escapeHtml(data.name)}</a>` : `<strong>${escapeHtml(data?.name || short)}</strong>`;
      }).join(', ') || 'Преподаватель не указан';
      const type = lessonType(entry.name);
      return `<div class="detail-variant">${type ? `<b>${escapeHtml(type)}</b><br>` : ''}${teacherLinks}</div>`;
    });
    $('#detail-teacher').innerHTML = teacherLines.join('<hr>');
    $('#detail-room').innerHTML = slot.entries.map((entry) => {
      const type = lessonType(entry.name);
      return `<div class="detail-variant">${type ? `<b>${escapeHtml(type)}</b>: ` : ''}${escapeHtml(formatEntryRooms(entry) || 'Аудитория не указана')}</div>`;
    }).join('');
    const weeks = [...new Set(slot.entries.map((entry) => entry.week).filter(Boolean))];
    $('#detail-week').textContent = weeks.includes('*') || !weeks.length ? 'Каждую неделю' : weeks.join(', ');
    $('#lesson-dialog').showModal();
  }
  function bindEvents() {
    $('#group-button').addEventListener('click', openGroupDialog);
    document.querySelectorAll('[data-view-toggle]').forEach((button) => button.addEventListener('click', () => switchView(button.dataset.viewToggle)));
    $('#lesson-dialog').addEventListener('click', (event) => { if (event.target === $('#lesson-dialog')) $('#lesson-dialog').close(); });
    $('#week-prev')?.addEventListener('click', () => { state.selectedDate = new Date(weekStart(state.selectedDate)); state.selectedDate.setUTCDate(state.selectedDate.getUTCDate() - 7); renderAll(); });
    $('#week-next')?.addEventListener('click', () => { state.selectedDate = new Date(weekStart(state.selectedDate)); state.selectedDate.setUTCDate(state.selectedDate.getUTCDate() + 7); renderAll(); });
    $('#quick-main').addEventListener('click', () => {
      const open = $('#quick-menu').classList.toggle('open');
      $('#quick-main').classList.toggle('active', open);
      $('#quick-main').setAttribute('aria-expanded', String(open));
    });
    $('#toggle-notifications')?.addEventListener('click', () => { closeQuickMenu(); toggleLessonReminders(); });
    $('#open-calendar').addEventListener('click', () => { closeQuickMenu(); renderCalendar(); $('#calendar-dialog').showModal(); });
    $('#open-map').addEventListener('click', async () => {
      closeQuickMenu();
      $('#map-dialog').showModal();
      await loadCampusDirectory();
      openCampusOnlineMap();
    });
    $('#campus-search')?.addEventListener('input', (event) => renderCampusDirectory(event.target.value));
    $('#campus-open-online')?.addEventListener('click', openCampusOnlineMap);
    $('#campus-basemap-osm')?.addEventListener('click', () => setCampusBasemap('osm'));
    $('#campus-basemap-satellite')?.addEventListener('click', () => setCampusBasemap('satellite'));
    $('#campus-mode-osm')?.addEventListener('click', () => {
      setCampusMapMode('online');
      if (campusOnlineMap) fitCampusMapToPark();
      else openCampusOnlineMap();
    });
    $('#campus-mode-yandex')?.addEventListener('click', openCampusYandexMap);
    $('#campus-fit-buildings')?.addEventListener('click', () => campusMapMode === 'yandex' ? fitCampusYandexMap() : fitCampusMapToBuildings());
    $('#open-weather').addEventListener('click', () => { closeQuickMenu(); $('#weather-dialog').showModal(); loadWeather(); });
    $('#toggle-theme').addEventListener('click', () => applyTheme(document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark', true));
    $('#calendar-prev').addEventListener('click', () => changeCalendarMonth(-1));
    $('#calendar-next').addEventListener('click', () => changeCalendarMonth(1));
    $('#group-search').addEventListener('input', (event) => renderGroupOptions(event.target.value));
    $('#today-button').addEventListener('click', () => { state.selectedDate = startOfDay(new Date()); renderAll(); });
    $('#day-strip').addEventListener('wheel', (event) => {
      if (Math.abs(event.deltaY) > Math.abs(event.deltaX)) {
        event.preventDefault();
        $('#day-strip').scrollBy({ left: event.deltaY, behavior: 'smooth' });
      }
    }, { passive: false });
    $('#refresh-button').addEventListener('click', () => loadData().then(() => showToast('Расписание обновлено')));
    $('#sync-bot-group').addEventListener('click', () => {
      if (!tg?.sendData || !state.schedule?.id) return;
      tg.sendData(JSON.stringify({ type: 'set_personal_group', group_id: String(state.schedule.id) }));
    });
    tg?.onEvent?.('themeChanged', () => document.documentElement.style.colorScheme = tg.colorScheme || 'light');
  }
  async function init() {
    registerOfflineSupport();
    window.addEventListener('offline', () => { updateConnectivityState(); showToast('Нет интернета. Используются сохранённые данные.'); });
    window.addEventListener('online', () => { updateConnectivityState(); showToast('Соединение восстановлено. Обновляем данные…'); loadData(); });
    updateConnectivityState();
    if (tg) {
      tg.ready();
      tg.expand();
      applyTheme(safeStorageGet(THEME_KEY) || 'light');
    }
    setGreeting();
    applyTheme(safeStorageGet(THEME_KEY) || 'light');
    if (isWeekPage) {
      document.body.classList.add('week-page');
      $('#welcome-date').textContent = 'РАСПИСАНИЕ';
      $('#greeting').textContent = 'Неделя';
      $('#week-view').classList.remove('hidden');
      $('#week-back').href = './index.html';
    } else {
      $('#welcome-date').textContent = formatDate(new Date(), { weekday: 'long', day: 'numeric', month: 'long' }).toUpperCase();
    }
    bindEvents();
    updateNotificationButton();
    updatePresence();
    window.setInterval(updatePresence, 45_000);
    await loadTeacherDirectory();
    await loadData();
    await syncLessonReminders();
    window.setInterval(() => { if (state.schedule) { renderHero(); renderSchedule(); } }, 60_000);
  }
  init();
})();
