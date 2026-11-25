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
  const state = { groups: [], schedule: null, selectedDate: startOfDay(new Date()), view: 'today', searchQuery: '' };
  const $ = (selector) => document.querySelector(selector);
  const tg = window.Telegram?.WebApp;

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
  function getLessons(date) {
    const lessons = state.schedule?.lessons?.[String(dayNumber(date))] || [];
    const parity = isOddWeek(date) ? 'нечёт' : 'чёт';
    return lessons
      .filter((lesson) => lesson.week === '*' || lesson.week === parity)
      .slice()
      .sort((a, b) => minutes(a.start) - minutes(b.start));
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
  function getStatus(date, lessons) {
    if (dateKey(date) !== dateKey(startOfDay(new Date()))) return { title: `${lessons.length} ${plural(lessons.length, 'пара', 'пары', 'пар')}`, subtitle: formatDate(date, { weekday: 'long', day: 'numeric', month: 'long' }) };
    const currentParts = moscowDateParts();
    const current = Number(currentParts.hour) * 60 + Number(currentParts.minute);
    const active = lessons.find((lesson) => current >= minutes(lesson.start) && current < minutes(lesson.end));
    if (active) return { title: 'Сейчас пара', subtitle: active.name };
    const next = lessons.find((lesson) => current < minutes(lesson.start));
    if (next) {
      const until = minutes(next.start) - current;
      const hint = until < 60 ? `через ${until} мин` : `в ${next.start}`;
      return { title: 'Следующая пара', subtitle: `${next.name} · ${hint}` };
    }
    return lessons.length ? { title: 'На сегодня всё', subtitle: 'Можно выдохнуть и отдохнуть 🌿' } : { title: 'Сегодня без пар', subtitle: 'Пусть день будет лёгким 🌿' };
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
    const lessons = getLessons(state.selectedDate);
    const today = dateKey(state.selectedDate) === dateKey(startOfDay(new Date()));
    const status = getStatus(state.selectedDate, lessons);
    $('#hero-label').textContent = today ? 'СЕГОДНЯ' : formatDate(state.selectedDate, { weekday: 'long' }).toUpperCase();
    $('#hero-title').textContent = status.title;
    $('#hero-subtitle').textContent = status.subtitle;
    $('#hero-footer-text').textContent = state.schedule?.updated ? `Обновлено ${state.schedule.updated}` : 'Расписание загружено';
    $('#week-label').textContent = `${isOddWeek(state.selectedDate) ? 'нечётная' : 'чётная'} неделя`;
    $('#welcome-date').textContent = today ? 'ТВОЙ УЧЕБНЫЙ ДЕНЬ' : formatDate(state.selectedDate, { day: 'numeric', month: 'long' }).toUpperCase();
    $('#schedule-heading').textContent = today ? 'Пары на сегодня' : 'Пары на выбранный день';
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
  function lessonMarkup(lesson, index, date) {
    const now = new Date();
    const nowParts = moscowDateParts(now);
    const currentMinutes = Number(nowParts.hour) * 60 + Number(nowParts.minute);
    const current = dateKey(date) === dateKey(startOfDay(now)) && currentMinutes >= minutes(lesson.start) && currentMinutes < minutes(lesson.end);
    const pair = lesson.pair ? `<small>${lesson.pair} пара</small>` : '';
    const teacher = lesson.teacher ? `<span class="meta-item"><span class="meta-icon">♙</span>${escapeHtml(lesson.teacher)}</span>` : '';
    const room = lesson.room ? `<span class="meta-item"><span class="meta-icon">⌖</span>${escapeHtml(lesson.room)}</span>` : '';
    const week = lesson.week && lesson.week !== '*' ? `<span class="week-badge">${escapeHtml(lesson.week)} нед.</span>` : '';
    return `<article class="lesson-card${current ? ' is-current' : ''}" data-lesson-index="${index}" style="animation-delay:${Math.min(index * 35, 175)}ms">${week}<div class="lesson-time">${escapeHtml(lesson.start)}<small>— ${escapeHtml(lesson.end)}</small>${pair}</div><div class="lesson-main">${current ? '<span class="current-tag">● Идёт сейчас</span>' : ''}<h3 class="lesson-title">${escapeHtml(lesson.name)}</h3><div class="lesson-meta">${teacher}${room}</div></div></article>`;
  }
  function renderSchedule() {
    const lessons = getLessons(state.selectedDate);
    const list = $('#schedule-list');
    if (!state.schedule) {
      list.innerHTML = '<div class="empty-card"><span class="empty-illustration">⌕</span><strong>Сначала выбери группу</strong><p>Найди свою группу, и мы покажем расписание на сегодня и всю неделю.</p></div>';
      $('#updated-note').textContent = '';
      return;
    }
    if (!lessons.length) {
      list.innerHTML = '<div class="empty-card"><span class="empty-illustration">☼</span><strong>Пар нет</strong><p>На этот день занятий не найдено. Проверь соседние дни или открой расписание в ЕИОС.</p></div>';
    } else {
      list.innerHTML = lessons.map((lesson, index) => lessonMarkup(lesson, index, state.selectedDate)).join('');
    }
    $('#updated-note').textContent = state.schedule.updated ? `Данные ЕИОС обновлены ${state.schedule.updated}` : 'Источник: расписание СПбГЛТУ';
    list.querySelectorAll('[data-lesson-index]').forEach((card) => {
      const open = () => openLessonDetails(lessons[Number(card.dataset.lessonIndex)], state.selectedDate);
      card.addEventListener('click', open);
      card.addEventListener('keydown', (event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); open(); } });
      card.setAttribute('tabindex', '0');
      card.setAttribute('role', 'button');
      card.setAttribute('aria-label', `Подробнее: ${lessons[Number(card.dataset.lessonIndex)].name}`);
    });
  }
  function renderWeek() {
    const monday = weekStart(state.selectedDate);
    const sunday = new Date(monday); sunday.setUTCDate(monday.getUTCDate() + 6);
    $('#week-range').textContent = `${formatDate(monday)} — ${formatDate(sunday)}`;
    $('#week-list').innerHTML = DAYS.map((day, index) => {
      const date = new Date(monday); date.setUTCDate(monday.getUTCDate() + index);
      const lessons = getLessons(date);
      const content = lessons.length ? lessons.map((lesson, lessonIndex) => `<button type="button" class="week-lesson" data-week-day="${day.id}" data-week-index="${lessonIndex}"><div class="week-lesson-time">${escapeHtml(lesson.start)}<br>${escapeHtml(lesson.end)}</div><div><p class="week-lesson-title">${escapeHtml(lesson.name)}</p><span class="week-lesson-meta">${[lesson.room, lesson.teacher].filter(Boolean).map(escapeHtml).join(' · ')}</span></div></button>`).join('') : '<p class="week-empty">Занятий нет</p>';
      const selected = dateKey(date) === dateKey(state.selectedDate);
      return `<article class="week-day"${selected ? ' style="border-color:#bfd493"' : ''}><div class="week-day-head"><span class="week-day-title">${day.full}<small>${formatDate(date, { day: 'numeric', month: 'short' })}</small></span><span class="week-day-count">${lessons.length ? `${lessons.length} ${plural(lessons.length, 'пара', 'пары', 'пар')}` : 'выходной'}</span></div>${content}</article>`;
    }).join('');
    $('#week-list').querySelectorAll('[data-week-day]').forEach((button) => button.addEventListener('click', () => {
      const date = new Date(monday); date.setUTCDate(monday.getUTCDate() + Number(button.dataset.weekDay) - 1);
      const lessons = getLessons(date);
      openLessonDetails(lessons[Number(button.dataset.weekIndex)], date);
    }));
  }
  function renderAll() {
    updateGroupLabel();
    renderHero();
    renderDays();
    renderSchedule();
    renderWeek();
    renderSearch();
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
      const response = await fetch(`../data/${encodeURIComponent(group.id)}.json`, { cache: 'no-cache' });
      if (!response.ok) throw new Error('schedule-not-found');
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
      const response = await fetch('../data/groups.json', { cache: 'no-cache' });
      if (!response.ok) throw new Error('groups-not-found');
      state.groups = await response.json();
      const saved = safeStorageGet(GROUP_KEY);
      const match = state.groups.find((group) => String(group.id) === String(saved));
      if (match) await selectGroup(match.id);
      else if (state.groups.length) { renderAll(); openGroupDialog(); }
      else throw new Error('no-groups');
    } catch {
      $('#hero-title').textContent = 'Расписание пока недоступно';
      $('#hero-subtitle').textContent = 'Проверь подключение и обнови экран';
      $('#hero-footer-text').textContent = 'Не удалось загрузить список групп';
      $('#schedule-list').innerHTML = '<div class="empty-card"><span class="empty-illustration">↻</span><strong>Не удалось загрузить данные</strong><p>Открой экран ещё раз, когда появится подключение к интернету.</p></div>';
    }
  }
  function switchView(view) {
    state.view = view;
    $('#today-view').classList.toggle('hidden', view !== 'today');
    $('#week-view').classList.toggle('hidden', view !== 'week');
    $('#search-view').classList.toggle('hidden', view !== 'search');
    document.querySelectorAll('.nav-item[data-view]').forEach((button) => {
      const active = button.dataset.view === `${view}-view`;
      button.classList.toggle('active', active);
      if (active) button.setAttribute('aria-current', 'page'); else button.removeAttribute('aria-current');
    });
    if (view === 'week') renderWeek();
    if (view === 'search') renderSearch();
  }

  function openLessonDetails(lesson, date) {
    if (!lesson) return;
    $('#detail-date').textContent = formatDate(date, { weekday: 'long', day: 'numeric', month: 'long' }).toUpperCase();
    $('#detail-name').textContent = lesson.name || 'Занятие';
    $('#detail-time').textContent = `${lesson.start} — ${lesson.end}${lesson.pair ? ` · ${lesson.pair} пара` : ''}`;
    $('#detail-teacher').textContent = lesson.teacher || 'Не указан';
    $('#detail-room').textContent = lesson.room || 'Не указана';
    $('#detail-week').textContent = lesson.week === '*' ? 'Каждую неделю' : lesson.week || 'Не указана';
    $('#lesson-dialog').showModal();
  }
  function renderSearch() {
    const results = $('#search-results');
    if (!state.schedule) { results.innerHTML = '<div class="empty-card"><strong>Сначала выбери группу</strong><p>Поиск работает по расписанию выбранной группы.</p></div>'; return; }
    const query = state.searchQuery.trim().toLocaleLowerCase('ru');
    const monday = weekStart(state.selectedDate);
    const matches = [];
    DAYS.forEach((day, index) => {
      const date = new Date(monday); date.setUTCDate(monday.getUTCDate() + index);
      getLessons(date).forEach((lesson) => {
        const haystack = [lesson.name, lesson.teacher, lesson.room].filter(Boolean).join(' ').toLocaleLowerCase('ru');
        if (!query || haystack.includes(query)) matches.push({ lesson, date, day });
      });
    });
    const sunday = new Date(monday); sunday.setUTCDate(monday.getUTCDate() + 6);
    $('#search-week-range').textContent = `${formatDate(monday)} — ${formatDate(sunday)}`;
    $('#search-hint').textContent = query ? `Найдено занятий: ${matches.length}` : 'Поиск по предмету, преподавателю или аудитории за выбранную неделю.';
    if (!matches.length) { results.innerHTML = '<div class="empty-card"><strong>Ничего не найдено</strong><p>Попробуй изменить запрос или выбрать другую неделю.</p></div>'; return; }
    results.innerHTML = matches.map(({ lesson, date, day }, index) => `<button type="button" class="search-result" data-result-index="${index}"><span class="search-result-time">${escapeHtml(lesson.start)}<span class="search-result-day">${day.short} · ${date.getUTCDate()}</span></span><span><span class="search-result-title">${escapeHtml(lesson.name)}</span><span class="search-result-meta">${escapeHtml([lesson.teacher, lesson.room].filter(Boolean).join(' · ') || 'Подробности не указаны')}</span></span></button>`).join('');
    results.querySelectorAll('[data-result-index]').forEach((button) => button.addEventListener('click', () => {
      const item = matches[Number(button.dataset.resultIndex)];
      openLessonDetails(item.lesson, item.date);
    }));
  }
  function bindEvents() {
    $('#group-button').addEventListener('click', openGroupDialog);
    $('#lesson-dialog').addEventListener('click', (event) => { if (event.target === $('#lesson-dialog')) $('#lesson-dialog').close(); });
    $('#schedule-search').addEventListener('input', (event) => { state.searchQuery = event.target.value; renderSearch(); });
    $('#search-prev-week').addEventListener('click', () => { state.selectedDate.setUTCDate(state.selectedDate.getUTCDate() - 7); renderAll(); });
    $('#search-next-week').addEventListener('click', () => { state.selectedDate.setUTCDate(state.selectedDate.getUTCDate() + 7); renderAll(); });
    $('#group-search').addEventListener('input', (event) => renderGroupOptions(event.target.value));
    $('#today-button').addEventListener('click', () => { state.selectedDate = startOfDay(new Date()); renderAll(); });
    $('#prev-week').addEventListener('click', () => { state.selectedDate.setUTCDate(state.selectedDate.getUTCDate() - 7); renderAll(); });
    $('#next-week').addEventListener('click', () => { state.selectedDate.setUTCDate(state.selectedDate.getUTCDate() + 7); renderAll(); });
    $('#refresh-button').addEventListener('click', () => loadData().then(() => showToast('Расписание обновлено')));
    $('#sync-bot-group').addEventListener('click', () => {
      if (!tg?.sendData || !state.schedule?.id) return;
      tg.sendData(JSON.stringify({ type: 'set_personal_group', group_id: String(state.schedule.id) }));
    });
    document.querySelectorAll('.nav-item[data-view]').forEach((button) => button.addEventListener('click', () => switchView(button.dataset.view.replace('-view', ''))));
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
    await loadData();
    window.setInterval(() => { if (state.schedule) { renderHero(); renderSchedule(); } }, 60_000);
  }
  init();
})();
