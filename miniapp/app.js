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
  const state = { groups: [], schedule: null, selectedDate: startOfDay(new Date()), weekAnchorDate: startOfDay(new Date()), view: 'today', searchQuery: '', filter: 'current', calendarMonth: new Date().getMonth(), calendarYear: new Date().getFullYear() };
  const $ = (selector) => document.querySelector(selector);
  const tg = window.Telegram?.WebApp;
  // Resolve data relative to the app script, not the page URL.
  const DATA_ROOT = new URL('../data/', document.currentScript?.src || new URL('miniapp/app.js', location.href)).href;
  const DATA_FALLBACK = 'https://raw.githubusercontent.com/DeWut/raspisanie.github.io/main/data/';
  async function fetchDataFile(path) {
    const candidates = [new URL(path, DATA_ROOT).href, new URL(path, DATA_FALLBACK).href];
    let lastError;
    for (const url of candidates) {
      try {
        const response = await fetch(url, { cache: 'no-cache' });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return response;
      } catch (error) { lastError = error; }
    }
    throw lastError || new Error('data-unavailable');
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
  function escapeHtml(value) {
    return String(value ?? '').replace(/[&<>"']/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[char]);
  }
  function minutes(value) { const [hour, minute] = String(value || '0:0').split(':').map(Number); return hour * 60 + minute; }
  function getLessons(date, filter = (state.view === 'week' ? state.filter : 'current')) {
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
    const match = String(name || '').trim().match(/\\s+(Лек\\.|Пр\\.|Лаб\\.)$/i);
    if (!match) return '';
    return ({ 'лек.': 'Лекция', 'пр.': 'Практика', 'лаб.': 'Лабораторная' })[match[1].toLowerCase()] || match[1];
  }
  function lessonBaseName(name) {
    return String(name || '').trim().replace(/\\s+(Лек\\.|Пр\\.|Лаб\\.)$/i, '').trim();
  }
  function compactLessonTitle(name) {
    const value = String(name || '').trim();
    if (/социально-ознакомительным практикумом/i.test(value) && /парк\\s+лта/i.test(value)) {
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
        title: active.pair ? `Идёт ${pairOrdinal(active.pair)} пару` : 'Идёт пара',
        subtitle: `${slotTitle(active)} · до ${active.end}`,
        next: next ? `Следующая: ${slotSummary(next)}` : 'После этой пары занятий больше нет',
      };
    }
    // University lunch window is fixed: 12:45–13:30.
    if (current >= 12 * 60 + 45 && current < 13 * 60 + 30) {
      return {
        title: 'Обед',
        subtitle: '12:45–13:30 · время отдохнуть и перекусить',
        next: next ? `Следующая: ${slotSummary(next)}` : 'На сегодня занятий больше нет',
      };
    }
    if (next) {
      const previous = slots.slice(0, nextIndex).at(-1);
      const nextPairLabel = next.pair ? `${pairOrdinal(next.pair)} пары` : 'следующей пары';
      const stateTitle = previous ? 'Перемена' : `До ${nextPairLabel}`;
      const stateSubtitle = previous
        ? `До ${nextPairLabel} · начнётся в ${next.start}`
        : (Number(next.pair) > 1 ? 'Первая пара сегодня не проводится по расписанию' : 'Пока можно спокойно подготовиться');
      return {
        title: stateTitle,
        subtitle: stateSubtitle,
        next: `Следующая: ${slotSummary(next)}`,
      };
    }
    return {
      title: slots.length ? 'Занятия закончились' : 'Сегодня без пар',
      subtitle: 'Сейчас время отдохнуть 🌿',
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
    $('#hero-footer-text').textContent = state.schedule?.updated ? `Обновлено ${state.schedule.updated}` : 'Расписание загружено';
    $('#week-label').textContent = `${isOddWeek(state.selectedDate) ? 'нечётная' : 'чётная'} неделя`;
    $('#welcome-date').textContent = today ? 'ТВОЙ УЧЕБНЫЙ ДЕНЬ' : formatDate(state.selectedDate, { day: 'numeric', month: 'long' }).toUpperCase();
    $('#schedule-heading').textContent = today ? 'Пары на сегодня' : formatDate(state.selectedDate, { day: 'numeric', month: 'long' });
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
    const weekValues = [...new Set(slot.entries.map((entry) => entry.week).filter((week) => week && week !== '*'))];
    const week = weekValues.length === 1 ? `<span class="week-badge">${escapeHtml(weekValues[0])} нед.</span>` : '';
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
    return `<article class="lesson-card${current ? ' is-current' : ''}${slot.entries.length > 1 ? ' has-variants' : ''}" data-slot-index="${index}" style="animation-delay:${Math.min(index * 35, 175)}ms">${week}<div class="lesson-time">${escapeHtml(slot.start)}<small>— ${escapeHtml(slot.end)}</small><small>${slot.pair ? `${escapeHtml(slot.pair)} пара` : ''}</small></div><div class="lesson-main">${current ? '<span class="current-tag">● Идёт сейчас</span>' : ''}<h3 class="lesson-title">${escapeHtml(commonTitle)}</h3><div class="lesson-variants">${entries}</div></div></article>`;
  }
  function renderSchedule() {
    const lessons = getLessons(state.selectedDate);
    const slots = groupLessonsBySlot(lessons);
    const list = $('#schedule-list');
    if (!state.schedule) {
      list.innerHTML = '<div class="empty-card"><span class="empty-illustration">✦</span><strong>Сначала выбери группу</strong><p>Найди свою группу, и мы покажем расписание на сегодня и всю неделю.</p></div>';
      $('#updated-note').textContent = '';
      $('#schedule-count').textContent = '';
      return;
    }
    if (!slots.length) {
      list.innerHTML = '<div class="empty-card"><span class="empty-illustration">☼</span><strong>Пар нет</strong><p>На этот день занятий не найдено. Проверь соседние дни или открой расписание в ЕИОС.</p></div>';
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
  function alignWeekToSelectedParity() {
    const anchor = weekStart(state.weekAnchorDate || state.selectedDate);
    if (isOddWeek(anchor) === (state.filter === 'odd')) { state.selectedDate = anchor; return; }
    const target = new Date(anchor);
    target.setUTCDate(target.getUTCDate() + 7);
    state.selectedDate = target;
  }
  function renderWeek() {
    const monday = weekStart(state.selectedDate);
    const sunday = new Date(monday); sunday.setUTCDate(monday.getUTCDate() + 6);
    const currentMonday = weekStart(startOfDay(new Date()));
    const isCurrentWeek = dateKey(monday) === dateKey(currentMonday);
    $('#week-range').textContent = `${isCurrentWeek ? 'Текущая неделя · ' : ''}${formatDate(monday)} — ${formatDate(sunday)}`;
    $('#week-current-button').classList.toggle('active', isCurrentWeek && state.filter === 'current');
    document.querySelectorAll('[data-filter]').forEach((button) => {
      const parity = isOddWeek(state.selectedDate) ? 'odd' : 'even';
      button.classList.toggle('active', button.dataset.filter === (state.filter === 'current' ? parity : state.filter));
    });
    $('#week-list').innerHTML = DAYS.map((day, index) => {
      const date = new Date(monday); date.setUTCDate(monday.getUTCDate() + index);
      const lessons = getLessons(date);
      const slots = groupLessonsBySlot(lessons);
      const content = slots.length ? slots.map((slot, slotIndex) => `<button type="button" class="week-lesson" data-week-day="${day.id}" data-week-index="${slotIndex}"><div class="week-lesson-time">${escapeHtml(slot.start)}<br>${escapeHtml(slot.end)}</div><div><p class="week-lesson-title">${escapeHtml(slotTitle(slot))}</p><span class="week-lesson-meta">${slot.entries.length > 1 ? `${slot.entries.length} варианта · ` : ''}${slot.entries.map((entry) => [entry.teacher, formatEntryRooms(entry)].filter(Boolean).join(' · ')).filter(Boolean).join(' / ')}</span></div></button>`).join('') : '<p class="week-empty">Занятий нет</p>';
      const selected = dateKey(date) === dateKey(state.selectedDate);
      return `<article class="week-day"${selected ? ' style="border-color:#bfd493"' : ''}><div class="week-day-head"><span class="week-day-title">${day.full}<small>${formatDate(date, { day: 'numeric', month: 'short' })}</small></span><span class="week-day-count">${slots.length ? `${slots.length} ${plural(slots.length, 'пара', 'пары', 'пар')}` : 'выходной'}</span></div>${content}</article>`;
    }).join('');
    $('#week-list').querySelectorAll('[data-week-day]').forEach((button) => button.addEventListener('click', () => {
      const date = new Date(monday); date.setUTCDate(monday.getUTCDate() + Number(button.dataset.weekDay) - 1);
      const slots = groupLessonsBySlot(getLessons(date));
      openLessonDetails(slots[Number(button.dataset.weekIndex)], date);
    }));
  }
  function renderAll() {
    updateGroupLabel();
    renderHero();
    renderDays();
    renderSchedule();
    renderWeek();
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
  function switchView(view) {
    const nextView = view === 'week' ? 'week' : 'today';
    if (nextView === 'week' && state.view !== 'week') state.weekAnchorDate = new Date(state.selectedDate);
    state.view = nextView;
    $('#today-view').classList.toggle('hidden', state.view !== 'today');
    $('#week-view').classList.toggle('hidden', state.view !== 'week');
    document.querySelectorAll('[data-view-toggle]').forEach((button) => {
      const active = button.dataset.viewToggle === state.view;
      button.classList.toggle('active', active);
      button.setAttribute('aria-pressed', String(active));
    });
    if (state.view === 'week') {
      if (state.filter !== 'current') alignWeekToSelectedParity();
      renderAll();
    }
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
      $('#calendar-details').innerHTML = `<strong>${escapeHtml(formatDate(date, { weekday: 'long', day: 'numeric', month: 'long' }))}</strong><div class="calendar-lesson-list">${lessons.length ? lessons.map((lesson) => `<p><b>${escapeHtml(lesson.start)}–${escapeHtml(lesson.end)}</b> ${escapeHtml(lesson.name)}<small>${escapeHtml(parseRoom(lesson.room))}</small></p>`).join('') : '<p>Пар нет</p>'}</div><button type="button" class="text-button" id="calendar-open-day">Открыть этот день</button>`;
      $('#calendar-open-day').addEventListener('click', () => {
        state.selectedDate = date;
        state.view = 'today';
        switchView('today');
        renderAll();
        $('#calendar-dialog').close();
      });
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
    document.querySelectorAll('[data-filter]').forEach((button) => button.addEventListener('click', () => {
      state.filter = button.dataset.filter;
      alignWeekToSelectedParity();
      renderAll();
    }));
    $('#week-current-button').addEventListener('click', () => {
      state.filter = 'current';
      state.selectedDate = startOfDay(new Date());
      state.weekAnchorDate = new Date(state.selectedDate);
      renderAll();
    });
    $('#quick-main').addEventListener('click', () => {
      const open = $('#quick-menu').classList.toggle('open');
      $('#quick-main').classList.toggle('active', open);
      $('#quick-main').setAttribute('aria-expanded', String(open));
    });
    $('#open-calendar').addEventListener('click', () => { closeQuickMenu(); renderCalendar(); $('#calendar-dialog').showModal(); });
    $('#open-map').addEventListener('click', () => { closeQuickMenu(); $('#map-dialog').showModal(); });
    $('#open-weather').addEventListener('click', () => { closeQuickMenu(); $('#weather-dialog').showModal(); loadWeather(); });
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
    if (tg) {
      tg.ready();
      tg.expand();
      tg.setHeaderColor?.('#173d2e');
      tg.setBackgroundColor?.('#f5f7f2');
    }
    setGreeting();
    $('#welcome-date').textContent = formatDate(new Date(), { weekday: 'long', day: 'numeric', month: 'long' }).toUpperCase();
    bindEvents();
    updatePresence();
    window.setInterval(updatePresence, 45_000);
    await loadTeacherDirectory();
    await loadData();
    window.setInterval(() => { if (state.schedule) { renderHero(); renderSchedule(); } }, 60_000);
  }
  init();
})();
