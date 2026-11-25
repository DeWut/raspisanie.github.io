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
  const state = { groups: [], schedule: null, selectedDate: startOfDay(new Date()), view: 'today', searchQuery: '', filter: 'all', calendarMonth: new Date().getMonth(), calendarYear: new Date().getFullYear() };
  const $ = (selector) => document.querySelector(selector);
  const tg = window.Telegram?.WebApp;
  const DATA_ROOT = location.pathname.includes('/miniapp/') ? '../data/' : './data/';
  const teachersDB = {
        "Васильев Н.П.":     { name: "Васильев Николай Павлович",       url: "https://spbftu.ru/allchairs/chairs-informatsionnykh-sistem-i-tekhnologiy/vasilev-nikolay-pavlovich" },
        "Мельников Н.Н.":    { name: "Мельников Николай Николаевич",    url: "https://spbftu.ru/allchairs/chairs-filosofii-i-sotsialnykh-distsiplin/melnikov-nikolay-nikolaevich" },
        "Вохмянин Н.А.":     { name: "Вохмянин Николай Александрович",  url: "https://spbftu.ru/allchairs/chairs-prikladnoy-mekhaniki-i-inzhenernoy-grafiki/vokhmyanin-nikolay-aleksandrovich" },
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
  function getLessons(date, filter = state.filter) {
    const lessons = state.schedule?.lessons?.[String(dayNumber(date))] || [];
    const parity = isOddWeek(date) ? 'нечёт' : 'чёт';
    return lessons.filter((lesson) => {
      if (filter === 'odd') return lesson.week === '*' || lesson.week === 'нечёт';
      if (filter === 'even') return lesson.week === '*' || lesson.week === 'чёт';
      if (filter === 'current') return lesson.week === '*' || lesson.week === parity;
      return true;
    }).slice().sort((a, b) => minutes(a.start) - minutes(b.start));
  }
  function parseRoom(raw) {
    if (!raw) return '';
    const cleaned = String(raw).replace(/^ауд\\.\\s*/i, '').trim();
    const match = cleaned.match(/^([^\\.]+)\\.(\\d)(\\d+)$/);
    if (!match) return cleaned;
    const floor = Number(match[2]) <= 1 ? 1 : Number(match[2]);
    return `${match[1]} корпус, ${floor} этаж, кабинет ${match[2]}${match[3]}`;
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
    const lessons = getLessons(state.selectedDate, 'current');
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
    const room = lesson.room ? `<span class="meta-item"><span class="meta-icon">⌖</span>${escapeHtml(parseRoom(lesson.room))}</span>` : '';
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
      const today = startOfDay(new Date());
      const showDay = state.filter !== 'current' || dateKey(date) >= dateKey(today);
      if (!showDay) return '';
      const visibleLessons = lessons;
      const content = visibleLessons.length ? visibleLessons.map((lesson, lessonIndex) => `<button type="button" class="week-lesson" data-week-day="${day.id}" data-week-index="${lessonIndex}"><div class="week-lesson-time">${escapeHtml(lesson.start)}<br>${escapeHtml(lesson.end)}</div><div><p class="week-lesson-title">${escapeHtml(lesson.name)}</p><span class="week-lesson-meta">${[lesson.room, lesson.teacher].filter(Boolean).map(escapeHtml).join(' · ')}</span></div></button>`).join('') : '<p class="week-empty">Занятий нет</p>';
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
      const response = await fetch(`${DATA_ROOT}${encodeURIComponent(group.id)}.json`, { cache: 'no-cache' });
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
      const response = await fetch(`${DATA_ROOT}groups.json`, { cache: 'no-cache' });
      if (!response.ok) throw new Error('groups-not-found');
      state.groups = await response.json();
      const saved = safeStorageGet(GROUP_KEY) || safeStorageGet('groupId');
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
  function openLessonDetails(lesson, date) {
    if (!lesson) return;
    $('#detail-date').textContent = formatDate(date, { weekday: 'long', day: 'numeric', month: 'long' }).toUpperCase();
    $('#detail-name').textContent = lesson.name || 'Занятие';
    $('#detail-time').textContent = `${lesson.start} — ${lesson.end}${lesson.pair ? ` · ${lesson.pair} пара` : ''}`;
    const teacherNames = String(lesson.teacher || '').split(',').map((name) => name.trim()).filter(Boolean);
    $('#detail-teacher').innerHTML = teacherNames.length ? teacherNames.map((short) => {
      const data = teachersDB[short];
      return data?.url ? `<a class="teacher-profile" href="${escapeHtml(data.url)}" target="_blank" rel="noopener">${escapeHtml(data.name)}</a>` : `<strong>${escapeHtml(data?.name || short)}</strong>`;
    }).join('<br>') : 'Не указан';
    $('#detail-room').textContent = parseRoom(lesson.room) || 'Не указана';
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
      getLessons(date, 'current').forEach((lesson) => {
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
    document.querySelectorAll('[data-filter]').forEach((button) => button.addEventListener('click', () => {
      state.filter = button.dataset.filter;
      document.querySelectorAll('[data-filter]').forEach((item) => item.classList.toggle('active', item.dataset.filter === state.filter));
      renderAll();
    }));
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
