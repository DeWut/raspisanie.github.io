/* map3d.js — campus map. Vendored MapLibre + local OSM/Overture campus data.
 * Basemaps: OpenFreeMap vector streets (scheme) and Esri World Imagery (satellite).
 * Neither basemap requires an API key. */
(function () {
  'use strict';

  var G = window.Map3D;
  var $ = function (id) { return document.getElementById(id); };
  var statusEl = $('status');

  function setStatus(text, isError) {
    if (!statusEl) return;
    if (!text) { statusEl.hidden = true; return; }
    statusEl.hidden = false;
    statusEl.textContent = text;
    statusEl.className = 'status' + (isError ? ' error' : '');
  }
  function esc(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
    });
  }

  if (!window.maplibregl || !G) {
    setStatus('Не загрузились библиотеки карты (MapLibre или map3d-geometry.js).', true);
    return;
  }
  setStatus('Загружаю данные кампуса…');

  var BASES = window.MAP3D_DATA_BASES || ['../data/', './data/', 'data/'];
  var FETCH_TIMEOUT_MS = 15000;
  var fetchLog = [];

  function fetchWithTimeout(url) {
    if (typeof AbortController === 'undefined') { return fetch(url, { cache: 'no-cache' }); }
    var ctl = new AbortController();
    var timer = setTimeout(function () { ctl.abort(); }, FETCH_TIMEOUT_MS);
    return fetch(url, { cache: 'no-cache', signal: ctl.signal }).then(function (r) {
      clearTimeout(timer);
      return r;
    }, function (err) {
      clearTimeout(timer);
      throw err;
    });
  }

  function loadJSON(name) {
    var i = 0;
    function next() {
      if (i >= BASES.length) { return Promise.reject(new Error('Не удалось загрузить ' + name)); }
      var url = BASES[i++] + name;
      setStatus('Загружаю ' + name + ' (' + i + '/' + BASES.length + ')…');
      return fetchWithTimeout(url).then(function (r) {
        if (!r.ok) { throw new Error('HTTP ' + r.status); }
        return r.json();
      }).catch(function (err) {
        fetchLog.push(url + ' — ' + (err && err.name === 'AbortError' ? 'нет ответа за ' + FETCH_TIMEOUT_MS / 1000 + ' с' : (err && err.message) || err));
        return next();
      });
    }
    return next();
  }

  var params = new URLSearchParams(location.search);
  var HOME = { center: [30.3399, 59.99455], zoom: 16.6, pitch: 60, bearing: -28 };
  var mode = 'sat';
  var buildings3D = false;
  var DEFAULT_MAP_MODE = 'scheme';
  var DEFAULT_BUILDINGS_3D = false;
  var satelliteCamera = null;
  var styleRequest = 0;
  var SCHEME_STYLE_URL = 'https://tiles.openfreemap.org/styles/bright';
  var ESRI_TILES = 'https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}';
  var EMPTY = { type: 'FeatureCollection', features: [] };
  function extrusion(id, source, minzoom, opacity) {
    var layer = {
      id: id, type: 'fill-extrusion', source: source,
      paint: {
        'fill-extrusion-color': ['get', 'col'],
        'fill-extrusion-base': ['get', 'z0'],
        'fill-extrusion-height': ['get', 'z1'],
        'fill-extrusion-opacity': opacity,
        'fill-extrusion-vertical-gradient': true
      }
    };
    if (minzoom) { layer.minzoom = minzoom; }
    return layer;
  }

  function buildStyle() {
    var layers = [
      { id: 'bg', type: 'background', paint: { 'background-color': '#dfe6d3' } },
      { id: 'sat', type: 'raster', source: 'esri', paint: { 'raster-fade-duration': 0, 'raster-opacity': 1 }, layout: { visibility: 'visible' } },
      { id: 'park', type: 'fill', source: 'ground', filter: ['==', ['get', 'kind'], 'park-boundary'],
        paint: { 'fill-color': '#cfe0b4', 'fill-opacity': 0.55 } },
      { id: 'green', type: 'fill', source: 'ground', filter: ['==', ['get', 'kind'], 'green-area'],
        paint: {
          'fill-color': ['match', ['get', 'areaType'], 'wood', '#86ad76', 'scrub', '#9bbd82', '#b4d08f'],
          'fill-opacity': 0.9
        } },
      { id: 'water', type: 'fill', source: 'ground', filter: ['==', ['get', 'kind'], 'water'],
        paint: { 'fill-color': '#8bb8d8' } },
      { id: 'path-casing', type: 'line', source: 'ground', filter: ['==', ['get', 'kind'], 'path'],
        layout: { 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': '#c9bfa5', 'line-width': ['interpolate', ['linear'], ['zoom'], 14, 0.8, 20, 2.4] } },
      { id: 'path', type: 'line', source: 'ground', filter: ['==', ['get', 'kind'], 'path'],
        layout: { 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': '#f4eedd', 'line-width': ['interpolate', ['linear'], ['zoom'], 14, 0.45, 20, 1.6] } },
      { id: 'campus-bld-2d', type: 'fill', source: 'footprints', layout: { visibility: 'none' },
        paint: { 'fill-color': ['get', 'col'], 'fill-opacity': 0.96, 'fill-outline-color': '#79766f' } },
      { id: 'bld-footprint', type: 'fill', source: 'ground', filter: ['==', ['get', 'kind'], 'building-footprint'],
        paint: { 'fill-color': ['get', 'col'], 'fill-opacity': 0.94, 'fill-outline-color': '#625e55' } },
      extrusion('bld', 'solid', 0, 1),
      { id: 'hl-flat', type: 'fill', source: 'hl', layout: { visibility: 'none' },
        paint: { 'fill-color': ['get', 'col'], 'fill-opacity': 0.24, 'fill-outline-color': '#e0ad35' } },
      extrusion('hl', 'hl', 0, 0.55)
    ];
    return {
      version: 8,
      sources: {
        esri: { type: 'raster', tiles: [ESRI_TILES], tileSize: 256, maxzoom: 19,
          attribution: 'Imagery © Esri, Maxar, Earthstar Geographics, GIS User Community' },
        ground: { type: 'geojson', data: EMPTY },
        footprints: { type: 'geojson', data: EMPTY, maxzoom: 20, tolerance: 0 },
        solid: { type: 'geojson', data: EMPTY, maxzoom: 20, tolerance: 0 },
        hl: { type: 'geojson', data: EMPTY, maxzoom: 20, tolerance: 0 }
      },
      layers: layers
    };
  }

  var map;
  try {
    map = new maplibregl.Map({
      container: 'map', style: buildStyle(),
      center: HOME.center, zoom: HOME.zoom, pitch: HOME.pitch, bearing: HOME.bearing,
      minZoom: 14, maxZoom: 20.5, maxPitch: 80,
      maxBounds: [[30.318, 59.982], [30.362, 60.004]],
      attributionControl: false
    });
  } catch (e) {
    setStatus('3D-карта требует WebGL, а он недоступен: ' + e.message, true);
    return;
  }
  map.addControl(new maplibregl.AttributionControl({ compact: true, customAttribution: '© OpenStreetMap contributors (ODbL) · © Overture Maps Foundation (ODbL)' }), 'bottom-right');
  map.addControl(new maplibregl.NavigationControl({ visualizePitch: true }), 'top-right');

  // Recalculate the map viewport after its parent navigation dialog becomes visible.
  window.addEventListener('resize', function () {
    if (map) map.resize();
  });

  function setBuildingMode(on) {
    buildings3D = !!on;
    var btn = $('btn-3d');
    if (btn) {
      btn.textContent = buildings3D ? 'Объёмы: Вкл' : 'Объёмы: Выкл';
      btn.setAttribute('aria-pressed', buildings3D ? 'true' : 'false');
    }
    if (map && map.getLayer('bld')) {
      map.setLayoutProperty('bld', 'visibility', buildings3D ? 'visible' : 'none');
    }
    if (map && map.getLayer('campus-bld-2d')) {
      map.setLayoutProperty('campus-bld-2d', 'visibility', buildings3D ? 'none' : 'visible');
    }
    if (map && map.getLayer('bld-footprint')) {
      map.setLayoutProperty('bld-footprint', 'visibility', buildings3D ? 'visible' : 'none');
    }
    if (map && map.getLayer('hl')) {
      map.setLayoutProperty('hl', 'visibility', buildings3D ? 'visible' : 'none');
    }
    if (map && map.getLayer('hl-flat')) {
      map.setLayoutProperty('hl-flat', 'visibility', buildings3D ? 'none' : 'visible');
    }
    // Flat vector scheme stays top-down; turning 3D volumes on tilts the camera.
    if (map && mode === 'scheme') {
      map.easeTo({ pitch: buildings3D ? 48 : 0, bearing: 0, duration: 500 });
    }
  }
  map.on('error', function (ev) { if (ev && ev.error && window.console) { console.warn('map error:', ev.error.message || ev.error); } });
  map.on('style.load', function () { if (scene) syncSceneData(); setBuildingMode(buildings3D); });

  function updateModeButton() {
    var button = $('btn-mode');
    if (button) {
      button.textContent = mode === 'sat' ? 'Схема' : 'Спутник';
      button.title = mode === 'sat' ? 'Переключить на векторную схему' : 'Переключить на спутниковую карту';
      button.setAttribute('aria-pressed', mode === 'scheme' ? 'true' : 'false');
    }
  }

  function addSchemeDataSourcesAndLayers(style) {
    var next = JSON.parse(JSON.stringify(style));
    next.sources = next.sources || {};
    next.sources.ground = { type: 'geojson', data: EMPTY };
    next.sources.footprints = { type: 'geojson', data: EMPTY, maxzoom: 20, tolerance: 0 };
    next.sources.solid = { type: 'geojson', data: EMPTY, maxzoom: 20, tolerance: 0 };
    next.sources.hl = { type: 'geojson', data: EMPTY, maxzoom: 20, tolerance: 0 };

    var campusLayers = [
      { id: 'campus-bld-2d', type: 'fill', source: 'footprints', layout: { visibility: 'none' },
        paint: { 'fill-color': ['get', 'col'], 'fill-opacity': 0.96, 'fill-outline-color': '#77736b' } },
      { id: 'bld-footprint', type: 'fill', source: 'ground', filter: ['==', ['get', 'kind'], 'building-footprint'],
        layout: { visibility: 'visible' },
        paint: { 'fill-color': ['get', 'col'], 'fill-opacity': 0.96, 'fill-outline-color': '#77736b' } },
      extrusion('bld', 'solid', 0, 1),
      { id: 'hl-flat', type: 'fill', source: 'hl', layout: { visibility: 'none' },
        paint: { 'fill-color': ['get', 'col'], 'fill-opacity': 0.24, 'fill-outline-color': '#d99e27' } },
      extrusion('hl', 'hl', 0, 0.55)
    ];
    var layers = next.layers || [];
    var firstSymbol = layers.findIndex(function (layer) { return layer.type === 'symbol'; });
    if (firstSymbol < 0) firstSymbol = layers.length;
    layers.splice.apply(layers, [firstSymbol, 0].concat(campusLayers));
    next.layers = layers;
    return next;
  }

  function syncSceneData() {
    if (!scene || !map || !map.isStyleLoaded()) return;
    var source = map.getSource('ground');
    if (source) source.setData({ type: 'FeatureCollection', features: scene.ground || [] });
    source = map.getSource('footprints');
    if (source) source.setData({ type: 'FeatureCollection', features: scene.footprints || [] });
    source = map.getSource('solid');
    if (source) source.setData({ type: 'FeatureCollection', features: scene.solid || [] });
    source = map.getSource('hl');
    if (source) source.setData(EMPTY);
    setBuildingMode(buildings3D);
  }

  function setMode(next) {
    if (next !== 'sat' && next !== 'scheme') return;
    var previous = mode;
    if (next === previous) { updateModeButton(); return; }
    mode = next;
    updateModeButton();
    var requestId = ++styleRequest;

    if (next === 'scheme') {
      satelliteCamera = { pitch: map.getPitch(), bearing: map.getBearing() };
      setStatus('Загружаю схему улиц, зданий и парков…');
      map.easeTo({ pitch: 0, bearing: 0, duration: 650 });
      fetch(SCHEME_STYLE_URL).then(function (response) {
        if (!response.ok) throw new Error('OpenFreeMap: HTTP ' + response.status);
        return response.json();
      }).then(function (style) {
        if (requestId !== styleRequest || mode !== 'scheme') return;
        map.once('style.load', function () {
          syncSceneData();
          setBuildingMode(buildings3D);
          setStatus('');
        });
        map.setStyle(addSchemeDataSourcesAndLayers(style), { diff: false });
      }).catch(function (error) {
        if (requestId !== styleRequest || mode !== 'scheme') return;
        mode = 'sat';
        updateModeButton();
        if (satelliteCamera) map.easeTo({ pitch: satelliteCamera.pitch, bearing: satelliteCamera.bearing, duration: 450 });
        setStatus('Не удалось загрузить векторную схему: ' + (error.message || error), true);
        setTimeout(function () { setStatus(''); }, 10000);
      });
      return;
    }

    setStatus('Возвращаю спутниковую подложку…');
    map.once('style.load', function () {
      syncSceneData();
      setBuildingMode(buildings3D);
      applyAtmosphere();
      setStatus('');
    });
    map.setStyle(buildStyle(), { diff: false });
    var camera = satelliteCamera || { pitch: HOME.pitch, bearing: HOME.bearing };
    map.easeTo({ pitch: camera.pitch, bearing: camera.bearing, duration: 650 });
  }

  var scene = null;
  var campusData = null;
  var popup = null;
  var campusMarkers = [];
  var wikipediaSummaryPromise = null;
  var WIKIPEDIA_PAGE = 'https://ru.wikipedia.org/wiki/Санкт-Петербургский_государственный_лесотехнический_университет';
  var WIKIPEDIA_API = 'https://ru.wikipedia.org/api/rest_v1/page/summary/Санкт-Петербургский_государственный_лесотехнический_университет';

  function loadWikipediaSummary() {
    if (!wikipediaSummaryPromise) {
      wikipediaSummaryPromise = fetch(WIKIPEDIA_API, { headers: { Accept: 'application/json' } })
        .then(function (r) { if (!r.ok) throw new Error('Wikipedia HTTP ' + r.status); return r.json(); })
        .then(function (data) {
          return { extract: String(data.extract || ''), url: data.content_urls && data.content_urls.desktop && data.content_urls.desktop.page || WIKIPEDIA_PAGE };
        })
        .catch(function () { return { extract: '', url: WIKIPEDIA_PAGE }; });
    }
    return wikipediaSummaryPromise;
  }

  function updateWikipediaBlock(container) {
    if (!container) return;
    loadWikipediaSummary().then(function (wiki) {
      if (!container.isConnected) return;
      var summary = container.querySelector('[data-wikipedia-summary]');
      var link = container.querySelector('[data-wikipedia-link]');
      if (summary) summary.textContent = wiki.extract || 'Краткая справка сейчас недоступна.';
      if (link) link.href = wiki.url || WIKIPEDIA_PAGE;
    });
  }

  function resolveBuildingPid(building) {
    if (!building || !building.osm || !scene) return null;
    var candidates = [building.osm.id, building.osm.outerWayId].filter(function (id) { return id != null; });
    for (var i = 0; i < candidates.length; i++) {
      if (scene.index[String(candidates[i])]) return String(candidates[i]);
    }
    return null;
  }

  function buildCampusMarkers() {
    campusMarkers.forEach(function (marker) { marker.remove(); });
    campusMarkers = [];
    var list = ((campusData && campusData.buildings) || []).filter(function (b) {
      return b && b.number && isFinite(Number(b.longitude)) && isFinite(Number(b.latitude));
    }).slice().sort(function (a, b) { return Number(a.number) - Number(b.number); });
    list.forEach(function (building) {
      var markerButton = document.createElement('button');
      markerButton.type = 'button';
      markerButton.className = 'campus-number-marker';
      markerButton.setAttribute('aria-label', 'Открыть корпус ' + building.number + ': ' + (building.name || 'учебный корпус'));
      markerButton.title = 'Корпус ' + building.number + ' · открыть сведения';
      var number = document.createElement('span');
      number.textContent = String(building.number);
      markerButton.appendChild(number);
      markerButton.addEventListener('click', function (event) {
        event.preventDefault();
        event.stopPropagation();
        setOrbit(false);
        var coordinate = [Number(building.longitude), Number(building.latitude)];
        map.flyTo({ center: coordinate, zoom: 18.4, pitch: buildings3D ? 58 : 0, bearing: buildings3D ? map.getBearing() : 0, duration: 900 });
        window.setTimeout(function () { selectCampusBuilding(building, coordinate); }, 650);
      });
      var marker = new maplibregl.Marker({ element: markerButton, anchor: 'bottom' })
        .setLngLat([Number(building.longitude), Number(building.latitude)])
        .addTo(map);
      marker.getElement().style.zIndex = '12';
      campusMarkers.push(marker);
    });
  }

  function applyAtmosphere() {
    try { map.setLight({ anchor: 'map', color: '#fff1d4', intensity: 0.5, position: [1.3, G.SUN_FROM_AZ, 42] }); } catch (e) { /* older/newer API */ }
    try {
      if (typeof map.setSky === 'function') {
        map.setSky({
          'sky-color': '#9ec7ee', 'horizon-color': '#eaf2f8', 'fog-color': '#e8eef2',
          'sky-horizon-blend': 0.5, 'horizon-fog-blend': 0.6, 'fog-ground-blend': 0.2
        });
      }
    } catch (e) { /* sky is optional; the CSS gradient behind the canvas is the fallback */ }
  }


  /* ---------- popup / selection ---------- */

  function popupHtml(p) {
    var camp = null;
    ((campusData && campusData.buildings) || []).forEach(function (b) {
      if (String(b.number) === String(p.campus) || (b.osm && (String(b.osm.id) === String(p.pid) || String(b.osm.outerWayId || '') === String(p.pid)))) { camp = b; }
    });
    var html = '<div class="pop">';
    var title = camp ? camp.name : (p.name || 'Здание');
    var heritage = camp && camp.heritage;
    var photo = heritage && heritage.photo;
    if (photo && photo.src) {
      var photoUrl = null;
      try { photoUrl = new URL(photo.src, location.href).href; } catch (e) { photoUrl = null; }
      if (photoUrl && /^https?:/.test(photoUrl)) {
        var photoSource = photo.sourceUrl && /^https:\/\/commons\.wikimedia\.org\//.test(photo.sourceUrl) ? photo.sourceUrl : '';
        html += '<figure class="campus-photo">';
        if (photoSource) html += '<a class="campus-photo-link" href="' + esc(photoSource) + '" target="_blank" rel="noopener noreferrer" aria-label="Открыть фотографию в Wikimedia Commons">';
        html += '<img src="' + esc(photoUrl) + '" alt="' + esc(photo.alt || title) + '" loading="lazy" decoding="async">';
        if (photoSource) html += '</a>';
        html += '<figcaption>' + esc(photo.credit || 'Фото Wikimedia Commons');
        if (photo.licenseUrl && /^https:\/\/creativecommons\.org\//.test(photo.licenseUrl)) html += ' · <a href="' + esc(photo.licenseUrl) + '" target="_blank" rel="noopener noreferrer">' + esc(photo.license || 'лицензия') + '</a>';
        html += '</figcaption></figure>';
      }
    }
    html += '<h3>' + esc(title) + '</h3>';
    if (camp && camp.description) { html += '<p>' + esc(camp.description) + '</p>'; }
    if (camp && camp.address) { html += '<p class="muted">' + esc(camp.address) + '</p>'; }
    if (heritage && heritage.heritageSourceUrl && /^https:\/\/commons\.wikimedia\.org\//.test(heritage.heritageSourceUrl)) {
      html += '<section class="wiki-card commons-card"><b>Wikimedia Commons</b><p>Фотографии и материалы об этом корпусе.</p><a href="' + esc(heritage.heritageSourceUrl) + '" target="_blank" rel="noopener noreferrer">Открыть материалы в Wikimedia Commons</a></section>';
    }
    html += '<section class="wiki-card"><b>Справка Wikipedia</b><p data-wikipedia-summary>Загружаем краткую справку…</p><a data-wikipedia-link href="' + esc(WIKIPEDIA_PAGE) + '" target="_blank" rel="noopener noreferrer">Открыть статью в Wikipedia</a></section>';
    if (p.hq === 'known') { html += '<p><b>Высота:</b> ' + esc(p.hm) + ' м (' + esc(p.hs || p.ds || 'указана в источнике') + ')</p>'; }
    else if (p.hq === 'levels') { html += '<p><b>Высота:</b> около ' + esc(p.hm) + ' м (' + esc(p.hs || 'оценка по числу этажей источника') + ')</p>'; }
    else if (p.hq === 'unknown') { html += '<p><b>3D-высота:</b> не указана в источнике; показан только реальный контур</p>'; }
    if (heritage) {
      if (heritage.built) html += '<p><b>Построен:</b> ' + esc(heritage.built) + '</p>';
      if (heritage.style) html += '<p><b>Стиль:</b> ' + esc(heritage.style) + '</p>';
      if (heritage.architects) html += '<p><b>Архитекторы:</b> ' + esc(heritage.architects) + '</p>';
      if (heritage.heritageSourceUrl && /^https:\/\/commons\.wikimedia\.org\//.test(heritage.heritageSourceUrl)) html += '<p><a href="' + esc(heritage.heritageSourceUrl) + '" target="_blank" rel="noopener noreferrer">Материалы Wikimedia Commons</a></p>';
      if (heritage.historySourceUrl && /^https?:\/\//.test(heritage.historySourceUrl)) html += '<p><a href="' + esc(heritage.historySourceUrl) + '" target="_blank" rel="noopener noreferrer">Историческая справка</a></p>';
    }
    if (camp && camp.units && camp.units.length) {
      html += '<ul>';
      camp.units.forEach(function (u) {
        html += '<li>' + esc(u.name) + (u.room ? ' <span class="muted">— ' + esc(u.room) + '</span>' : '') + '</li>';
      });
      html += '</ul>';
    }
    if (camp && camp.units && camp.units.length) html += '<p><button type="button" class="floor-open" data-open-floorplan="' + esc(camp.number) + '">Схема этажей и кабинеты</button></p>';
    if (p.src && /^https:\/\/www\.openstreetmap\.org\//.test(p.src)) {
      html += '<p class="muted"><a href="' + esc(p.src) + '" target="_blank" rel="noopener">Объект в OpenStreetMap</a></p>';
    }
    return html + '</div>';
  }

  function clearSelection() {
    var old = popup;
    popup = null;
    map.getSource('hl').setData(EMPTY);
    if (old) { old.remove(); }
  }

  function floorForRoom(room) {
    var value = String(room || '').trim();
    var explicit = value.match(/(?:^|\b)([1-9])\s*[- ]?й\s*этаж/i);
    if (explicit) return Number(explicit[1]);
    var roomNumber = value.match(/\b([1-9])\d{2,3}\b/);
    return roomNumber ? Number(roomNumber[1]) : null;
  }

  var activeFloorBuilding = null;
  var activeFloorNumber = 1;
  var floorPlanZoom = 1;
  var roomEditor = { placing: false, start: null, draft: null, editingId: null, movingId: null };
  var ROOM_OVERRIDES_KEY = 'spbftu-floorplan-manual-rooms-v1';

  function readManualRooms() {
    try {
      var value = JSON.parse(localStorage.getItem(ROOM_OVERRIDES_KEY) || '{}');
      return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
    } catch (error) { return {}; }
  }

  function writeManualRooms(value) {
    try {
      localStorage.setItem(ROOM_OVERRIDES_KEY, JSON.stringify(value));
      return true;
    } catch (error) { return false; }
  }

  function manualRoomKey(building, floor) {
    return String(building.number) + ':' + String(floor);
  }

  function roomsForFloor(building, floor) {
    var all = readManualRooms();
    var value = all[manualRoomKey(building, floor)];
    return Array.isArray(value) ? value : [];
  }

  function manualRoomsSvg(building, floor, svg) {
    var items = roomsForFloor(building, floor);
    if (!items.length) return svg;
    var markup = '<g class="fp-user-rooms">';
    items.forEach(function (room) {
      var x = Number(room.x), y = Number(room.y), w = Number(room.w), h = Number(room.h);
      if (![x,y,w,h].every(Number.isFinite) || w < 12 || h < 12) return;
      markup += '<g class="fp-user-room" data-user-room-id="' + esc(room.id) + '" role="button" tabindex="0" aria-label="Изменить кабинет ' + esc(room.number) + '">';
      markup += '<title>' + esc(room.number) + (room.name ? ' · ' + esc(room.name) : '') + '</title>';
      markup += '<rect x="' + x + '" y="' + y + '" width="' + w + '" height="' + h + '" rx="3"/>';
      markup += '<text x="' + (x+w/2) + '" y="' + (y+h/2+5) + '">' + esc(room.number) + '</text>';
      if (room.name) markup += '<text class="fp-user-room-name" x="' + (x+w/2) + '" y="' + (y+h/2+19) + '">' + esc(room.name) + '</text>';
      markup += '</g>';
    });
    markup += '</g>';
    return svg.replace('</svg>', markup + '</svg>');
  }

  function roomEditorMarkup() {
    var editing = !!roomEditor.editingId;
    return '<section class="room-editor">' +
      '<div class="room-editor-heading"><b>Редактор кабинетов</b><span>Ваши добавления сохраняются на этом устройстве</span></div>' +
      '<div class="room-editor-fields">' +
      '<label>Номер кабинета<input data-room-number type="text" maxlength="24" placeholder="Например, 215" value="' + esc(roomEditor.draft && roomEditor.draft.number || '') + '"></label>' +
      '<label>Название / примечание<input data-room-name type="text" maxlength="100" placeholder="Например, кафедра физики" value="' + esc(roomEditor.draft && roomEditor.draft.name || '') + '"></label>' +
      '</div>' +
      '<div class="room-editor-actions">' +
      '<button type="button" data-room-place>' + (roomEditor.movingId ? 'Перерисовать место' : 'Нарисовать на плане') + '</button>' +
      (editing ? '<button type="button" data-room-save>Сохранить изменения</button><button type="button" data-room-delete class="danger">Удалить кабинет</button>' : '') +
      '<button type="button" data-room-cancel' + (editing || roomEditor.placing ? '' : ' hidden') + '>Отмена</button>' +
      '</div>' +
      '<div class="room-editor-tools"><button type="button" data-room-export>Экспорт JSON</button><button type="button" data-room-import>Импорт JSON</button><input data-room-file type="file" accept="application/json,.json" hidden></div>' +
      '<p class="room-editor-status" data-room-editor-status>' + (roomEditor.placing ? 'Проведите пальцем по плану, чтобы нарисовать прямоугольник кабинета.' : 'Введите номер и название, нажмите «Нарисовать на плане» и обведите комнату пальцем. Нажмите на добавленную комнату, чтобы изменить её.') + '</p>' +
      '<p class="room-editor-note">Личные изменения не видны другим пользователям. Нажмите «Экспорт JSON», чтобы сохранить файл и передать мне для включения кабинетов в общую версию.</p>' +
      '</section>';
  }

  function setRoomEditorStatus(text) {
    var status = $('floorplan-content') && $('floorplan-content').querySelector('[data-room-editor-status]');
    if (status) status.textContent = text;
  }

  function roomPoint(event, svg) {
    var rect = svg.getBoundingClientRect();
    var viewBox = (svg.getAttribute('viewBox') || '0 0 1000 760').trim().split(/[ ,]+/).map(Number);
    return { x: viewBox[0] + (event.clientX - rect.left) * viewBox[2] / rect.width,
      y: viewBox[1] + (event.clientY - rect.top) * viewBox[3] / rect.height,
      width: viewBox[2], height: viewBox[3] };
  }

  function roomPreview(svg, x, y, w, h) {
    var ns = 'http://www.w3.org/2000/svg';
    var group = svg.querySelector('.fp-room-preview');
    if (!group) {
      group = document.createElementNS(ns, 'g');
      group.setAttribute('class', 'fp-room-preview');
      var rect = document.createElementNS(ns, 'rect');
      rect.setAttribute('class', 'fp-room-preview-rect');
      group.appendChild(rect);
      svg.appendChild(group);
    }
    var shape = group.querySelector('rect');
    shape.setAttribute('x', x); shape.setAttribute('y', y);
    shape.setAttribute('width', w); shape.setAttribute('height', h);
  }

  function exportManualRooms() {
    var data = readManualRooms();
    var blob = new Blob([JSON.stringify({ version: 1, exportedAt: new Date().toISOString(), rooms: data }, null, 2)], { type: 'application/json' });
    var url = URL.createObjectURL(blob);
    var link = document.createElement('a');
    link.href = url; link.download = 'spbftu-campus-rooms.json';
    document.body.appendChild(link); link.click(); link.remove();
    setTimeout(function () { URL.revokeObjectURL(url); }, 1000);
  }

  function importManualRooms(file) {
    if (!file) return;
    var reader = new FileReader();
    reader.onload = function () {
      try {
        var parsed = JSON.parse(String(reader.result || '{}'));
        var source = parsed.rooms && typeof parsed.rooms === 'object' ? parsed.rooms : parsed;
        var clean = {};
        Object.keys(source).forEach(function (key) {
          if (!/^\\d+:[1-9]$/.test(key) || !Array.isArray(source[key])) return;
          clean[key] = source[key].filter(function (item) {
            return item && String(item.number || '').trim() && [item.x,item.y,item.w,item.h].every(function (v) { return Number.isFinite(Number(v)); });
          }).map(function (item) {
            return { id: String(item.id || Date.now() + Math.random()), number: String(item.number), name: String(item.name || ''),
              x: Number(item.x), y: Number(item.y), w: Number(item.w), h: Number(item.h) };
          });
        });
        if (!Object.keys(clean).length) throw new Error('В JSON не найдены записи кабинетов');
        if (!writeManualRooms(clean)) throw new Error('Браузер запретил сохранение данных');
        roomEditor.editingId = null; roomEditor.movingId = null; roomEditor.placing = false;
        renderFloorPlan(activeFloorBuilding, activeFloorNumber);
        setRoomEditorStatus('Импортировано. Кабинеты сохранены на этом устройстве.');
      } catch (error) { setRoomEditorStatus('Не удалось импортировать: ' + error.message); }
    };
    reader.onerror = function () { setRoomEditorStatus('Не удалось прочитать JSON-файл.'); };
    reader.readAsText(file);
  }

  function renderFloorPlan(building, floor) {
    var content = $('floorplan-content');
    if (!content) return;
    activeFloorBuilding = building;
    activeFloorNumber = floor;
    floorPlanZoom = 1;
    var custom = window.CAMPUS_FLOORPLANS && window.CAMPUS_FLOORPLANS[String(building.number)];
    if (custom && custom.floors && custom.floors[floor]) {
      var plan = custom.floors[floor];
      var tabs = '';
      Object.keys(custom.floors).sort(function (a,b) { return Number(a)-Number(b); }).forEach(function (key) {
        tabs += '<button type="button" class="floor-tab' + (Number(key) === Number(floor) ? ' active' : '') + '" data-floor-select="' + key + '" aria-pressed="' + (Number(key) === Number(floor) ? 'true' : 'false') + '">' + key + ' этаж</button>';
      });
      var assigned = [];
      (building.units || []).forEach(function (unit) {
        if (floorForRoom(unit.room) === Number(floor)) assigned.push(unit);
      });
      var html = '<nav class="floor-tabs" aria-label="Выбор этажа">' + tabs + '</nav>';
      html += '<div class="floor-toolbar"><span>' + esc(plan.title) + '</span><div><button type="button" data-floor-zoom="-1" aria-label="Уменьшить схему">−</button><button type="button" data-floor-zoom="1" aria-label="Увеличить схему">+</button><button type="button" data-floor-zoom="reset">Сброс</button></div></div>';
      var svgWithRooms = manualRoomsSvg(building, floor, plan.svg);
      html += '<div class="floorplan-viewport"><div class="floorplan-drawing" style="width:100%">' + svgWithRooms + '</div></div>';
      html += roomEditorMarkup();
      html += '<section class="floor-units"><h4>Подразделения и кабинеты этого этажа</h4>';
      if (assigned.length) {
        html += '<div class="room-grid">';
        assigned.forEach(function (unit) {
          html += '<article class="room-card"><b>' + esc(unit.room || 'Этаж ' + floor) + '</b><span>' + esc(unit.name) + '</span></article>';
        });
        html += '</div>';
      } else {
        html += '<p class="muted">Подтверждённые номера кабинетов для этого этажа пока не внесены.</p>';
      }
      html += '</section><p class="floor-note">' + esc(custom.note) + '</p>';
      content.innerHTML = html;
      var drawing = content.querySelector('.floorplan-drawing');
      if (drawing) drawing.style.width = (floorPlanZoom * 100) + '%';
      return;
    }

    var units = building.units || [];
    var floors = {};
    var unassigned = [];
    units.forEach(function (unit) {
      var f = floorForRoom(unit.room);
      if (f == null) unassigned.push(unit);
      else (floors[f] || (floors[f] = [])).push(unit);
    });
    var keys = Object.keys(floors).sort(function (a, b) { return Number(a) - Number(b); });
    var htmlFallback = '';
    keys.forEach(function (f) {
      htmlFallback += '<section class="floor-section"><h4>' + f + '-й этаж</h4><div class="room-grid">';
      floors[f].forEach(function (unit) {
        htmlFallback += '<article class="room-card"><b>' + esc(unit.room) + '</b><span>' + esc(unit.name) + '</span></article>';
      });
      htmlFallback += '</div></section>';
    });
    if (unassigned.length) {
      htmlFallback += '<section class="floor-section"><h4>Этаж не указан в данных</h4><div class="room-grid">';
      unassigned.forEach(function (unit) { htmlFallback += '<article class="room-card"><b>' + esc(unit.room || '—') + '</b><span>' + esc(unit.name) + '</span></article>'; });
      htmlFallback += '</div></section>';
    }
    htmlFallback += '<p class="floor-note">Это справочная схема размещения подразделений по номерам аудиторий; точная геометрия помещений пока не подтверждена.</p>';
    content.innerHTML = htmlFallback || '<p>Для этого корпуса пока нет опубликованных данных о кабинетах.</p>';
  }

  function openFloorPlan(number) {
    var building = ((campusData && campusData.buildings) || []).find(function (b) { return String(b.number) === String(number); });
    if (!building) return;
    var panel = $('floorplan');
    var title = $('floorplan-title');
    if (!panel) return;
    title.textContent = building.name + ' · ' + (building.address || '');
    var custom = window.CAMPUS_FLOORPLANS && window.CAMPUS_FLOORPLANS[String(building.number)];
    var firstFloor = custom ? 1 : (function () {
      var found = (building.units || []).map(function (u) { return floorForRoom(u.room); }).filter(Boolean).sort(function(a,b){return a-b;});
      return found.length ? found[0] : 1;
    })();
    renderFloorPlan(building, firstFloor);
    panel.hidden = false;
    panel.setAttribute('aria-hidden', 'false');
  }

  function closeFloorPlan() {
    var panel = $('floorplan');
    if (panel) { panel.hidden = true; panel.setAttribute('aria-hidden', 'true'); }
    roomEditor.placing = false; roomEditor.start = null; roomEditor.editingId = null; roomEditor.movingId = null;
  }

  function showBuildingPopup(info, lngLat) {
    if (!info) return;
    if (popup) { var old = popup; popup = null; old.remove(); }
    var highlight = info.pid && scene && scene.highlight[info.pid] ? scene.highlight[info.pid] : [];
    if (map.getSource('hl')) map.getSource('hl').setData({ type: 'FeatureCollection', features: highlight });
    var mine = new maplibregl.Popup({ closeButton: true, maxWidth: '340px', className: 'bpop', focusAfterOpen: false })
      .setLngLat(lngLat).setHTML(popupHtml(info)).addTo(map);
    popup = mine;
    updateWikipediaBlock(mine.getElement());
    var floorButton = mine.getElement().querySelector('[data-open-floorplan]');
    if (floorButton) floorButton.addEventListener('click', function () { openFloorPlan(floorButton.getAttribute('data-open-floorplan')); });
    mine.on('close', function () {
      if (popup === mine) {
        popup = null;
        if (map.getSource('hl')) map.getSource('hl').setData(EMPTY);
      }
    });
  }

  function selectBuilding(pid, lngLat) {
    var info = scene && scene.index[pid];
    if (info) showBuildingPopup(info, lngLat);
  }

  function selectCampusBuilding(building, lngLat) {
    if (!building) return;
    var pid = resolveBuildingPid(building);
    var info = pid && scene && scene.index[pid] ? scene.index[pid] : {
      pid: pid || '',
      campus: String(building.number),
      name: building.name || ('Корпус № ' + building.number),
      hq: 'unknown'
    };
    info.campus = String(building.number);
    showBuildingPopup(info, lngLat);
  }

  map.on('click', function (e) {
    var hits = map.queryRenderedFeatures(e.point, { layers: ['bld', 'campus-bld-2d', 'bld-footprint'] });
    if (!hits.length) { clearSelection(); return; }
    selectBuilding(String(hits[0].properties.pid), e.lngLat);
  });
  map.on('mousemove', function (e) {
    var hits = map.queryRenderedFeatures(e.point, { layers: ['bld', 'campus-bld-2d', 'bld-footprint'] });
    map.getCanvas().style.cursor = hits.length ? 'pointer' : '';
  });

  /* ---------- orbit ---------- */

  var orbiting = false;
  function orbitStep() {
    if (!orbiting) { return; }
    map.setBearing(map.getBearing() + 0.07);
    requestAnimationFrame(orbitStep);
  }
  function setOrbit(on) {
    orbiting = on;
    var btn = $('btn-orbit');
    if (btn) {
      btn.setAttribute('aria-pressed', on ? 'true' : 'false');
      btn.textContent = on ? 'Облёт: Вкл' : 'Облёт';
      btn.title = on ? 'Остановить автоматический облёт' : 'Запустить автоматический облёт';
    }
    if (on) { requestAnimationFrame(orbitStep); }
  }
  ['pointerdown', 'wheel', 'touchstart'].forEach(function (name) {
    $('map').addEventListener(name, function () { if (orbiting) { setOrbit(false); } }, { passive: true });
  });

  /* ---------- UI wiring ---------- */

  function wire() {
    var controls = $('tools');
    var controlsButton = $('btn-controls');
    if (controlsButton && controls) {
      controlsButton.addEventListener('click', function (event) {
        event.preventDefault();
        event.stopPropagation();
        controls.hidden = !controls.hidden;
        controlsButton.setAttribute('aria-expanded', controls.hidden ? 'false' : 'true');
      });
    }
    if (controls) {
      controls.addEventListener('click', function (event) {
        var btn = event.target.closest('button');
        if (!btn || !controls.contains(btn)) return;
        event.preventDefault();
        event.stopPropagation();
        switch (btn.id) {
          case 'btn-mode': setMode(mode === 'sat' ? 'scheme' : 'sat'); break;
          case 'btn-3d': setBuildingMode(!buildings3D); break;
          case 'btn-orbit': setOrbit(!orbiting); break;
          case 'btn-home':
            setOrbit(false);
            map.flyTo({ center: HOME.center, zoom: HOME.zoom, pitch: mode === 'scheme' ? (buildings3D ? 48 : 0) : HOME.pitch, bearing: mode === 'scheme' ? 0 : HOME.bearing, duration: 1800 });
            break;
        }
      });
    }
    var floorClose = $('floorplan-close');
    if (floorClose) floorClose.addEventListener('click', closeFloorPlan);
    var floorContent = $('floorplan-content');
    if (floorContent) floorContent.addEventListener('click', function (event) {
      var tab = event.target.closest('[data-floor-select]');
      if (tab && activeFloorBuilding) {
        roomEditor.placing = false; roomEditor.editingId = null; roomEditor.movingId = null;
        renderFloorPlan(activeFloorBuilding, Number(tab.getAttribute('data-floor-select')));
        return;
      }
      var addedRoom = event.target.closest('[data-user-room-id]');
      if (addedRoom && activeFloorBuilding) {
        var items = roomsForFloor(activeFloorBuilding, activeFloorNumber);
        var found = items.find(function (item) { return String(item.id) === String(addedRoom.getAttribute('data-user-room-id')); });
        if (found) {
          roomEditor.editingId = String(found.id); roomEditor.movingId = String(found.id);
          roomEditor.draft = { number: found.number, name: found.name || '' };
          var editor = floorContent.querySelector('.room-editor');
          if (editor) {
            var numberInput = editor.querySelector('[data-room-number]'), nameInput = editor.querySelector('[data-room-name]');
            if (numberInput) numberInput.value = found.number;
            if (nameInput) nameInput.value = found.name || '';
            var save = editor.querySelector('[data-room-save]'), del = editor.querySelector('[data-room-delete]'), cancel = editor.querySelector('[data-room-cancel]');
            if (save) save.hidden = false; if (del) del.hidden = false; if (cancel) cancel.hidden = false;
            var place = editor.querySelector('[data-room-place]'); if (place) place.textContent = 'Перерисовать место';
            setRoomEditorStatus('Выбран кабинет ' + found.number + '. Измените подпись, перерисуйте место или удалите запись.');
          }
        }
        return;
      }
      var zoom = event.target.closest('[data-floor-zoom]');
      if (zoom) {
        var action = zoom.getAttribute('data-floor-zoom');
        floorPlanZoom = action === 'reset' ? 1 : Math.max(1, Math.min(2.5, floorPlanZoom + Number(action) * 0.25));
        var drawing = floorContent.querySelector('.floorplan-drawing');
        if (drawing) drawing.style.width = (floorPlanZoom * 100) + '%';
        return;
      }
      if (event.target.closest('[data-room-place]')) {
        var editorBox = floorContent.querySelector('.room-editor');
        var num = editorBox && editorBox.querySelector('[data-room-number]');
        var name = editorBox && editorBox.querySelector('[data-room-name]');
        var numberValue = num ? num.value.trim() : '';
        if (!numberValue) { setRoomEditorStatus('Сначала укажите номер кабинета.'); if (num) num.focus(); return; }
        roomEditor.draft = { number: numberValue, name: name ? name.value.trim() : '' };
        roomEditor.placing = true; roomEditor.start = null;
        var svg = floorContent.querySelector('.floorplan-svg');
        if (svg) svg.classList.add('is-placing-room');
        setRoomEditorStatus('Проведите пальцем или мышью по плану, чтобы обвести кабинет. Короткое нажатие поставит стандартную метку.');
        return;
      }
      if (event.target.closest('[data-room-cancel]')) {
        roomEditor.placing = false; roomEditor.start = null; roomEditor.draft = null; roomEditor.editingId = null; roomEditor.movingId = null;
        renderFloorPlan(activeFloorBuilding, activeFloorNumber); return;
      }
      if (event.target.closest('[data-room-save]')) {
        var box = floorContent.querySelector('.room-editor');
        var numberInput = box && box.querySelector('[data-room-number]');
        var nameInput = box && box.querySelector('[data-room-name]');
        var number = numberInput ? numberInput.value.trim() : '';
        if (!number) { setRoomEditorStatus('Номер кабинета обязателен.'); return; }
        var all = readManualRooms(), key = manualRoomKey(activeFloorBuilding, activeFloorNumber);
        all[key] = (all[key] || []).map(function (item) {
          if (String(item.id) !== String(roomEditor.editingId)) return item;
          item.number = number; item.name = nameInput ? nameInput.value.trim() : '';
          return item;
        });
        if (!writeManualRooms(all)) { setRoomEditorStatus('Не удалось сохранить. Проверьте доступное место в хранилище браузера.'); return; }
        roomEditor.editingId = null; roomEditor.movingId = null; roomEditor.draft = null;
        renderFloorPlan(activeFloorBuilding, activeFloorNumber); setRoomEditorStatus('Изменения сохранены.');
        return;
      }
      if (event.target.closest('[data-room-delete]')) {
        var allRooms = readManualRooms(), roomKey = manualRoomKey(activeFloorBuilding, activeFloorNumber);
        allRooms[roomKey] = (allRooms[roomKey] || []).filter(function (item) { return String(item.id) !== String(roomEditor.editingId); });
        writeManualRooms(allRooms);
        roomEditor.editingId = null; roomEditor.movingId = null; roomEditor.draft = null;
        renderFloorPlan(activeFloorBuilding, activeFloorNumber); setRoomEditorStatus('Кабинет удалён.');
        return;
      }
      if (event.target.closest('[data-room-export]')) { exportManualRooms(); setRoomEditorStatus('Файл JSON подготовлен. Он содержит все ручные кабинеты с этого устройства.'); return; }
      if (event.target.closest('[data-room-import]')) { var file = floorContent.querySelector('[data-room-file]'); if (file) file.click(); return; }
    });
    if (floorContent) {
      floorContent.addEventListener('change', function (event) {
        if (event.target.matches('[data-room-file]')) {
          importManualRooms(event.target.files && event.target.files[0]);
          event.target.value = '';
        }
      });
      floorContent.addEventListener('pointerdown', function (event) {
        if (!roomEditor.placing || event.target.closest('[data-user-room-id]')) return;
        var svg = event.target.closest('svg.floorplan-svg');
        if (!svg) return;
        var point = roomPoint(event, svg);
        roomEditor.start = { svg: svg, point: point };
        svg.classList.add('is-placing-room');
        try { svg.setPointerCapture(event.pointerId); } catch (error) {}
        event.preventDefault();
      });
      floorContent.addEventListener('pointermove', function (event) {
        if (!roomEditor.placing || !roomEditor.start) return;
        var svg = roomEditor.start.svg, a = roomEditor.start.point, b = roomPoint(event, svg);
        var x = Math.min(a.x,b.x), y = Math.min(a.y,b.y), w = Math.abs(a.x-b.x), h = Math.abs(a.y-b.y);
        roomPreview(svg,x,y,Math.max(1,w),Math.max(1,h));
      });
      floorContent.addEventListener('pointerup', function (event) {
        if (!roomEditor.placing || !roomEditor.start) return;
        var svg = roomEditor.start.svg, a = roomEditor.start.point, b = roomPoint(event, svg);
        var width = Math.abs(a.x-b.x), height = Math.abs(a.y-b.y);
        if (width < 18 || height < 18) { width = 78; height = 48; }
        var x = Math.max(8, Math.min(a.width-width-8, Math.min(a.x,b.x) - (Math.abs(a.x-b.x)<18 ? width/2 : 0)));
        var y = Math.max(88, Math.min(a.height-50-height, Math.min(a.y,b.y) - (Math.abs(a.y-b.y)<18 ? height/2 : 0)));
        var all = readManualRooms(), key = manualRoomKey(activeFloorBuilding, activeFloorNumber);
        var entries = all[key] || [];
        if (roomEditor.movingId) {
          entries = entries.map(function(item) {
            if (String(item.id) !== String(roomEditor.movingId)) return item;
            item.x=x; item.y=y; item.w=width; item.h=height; return item;
          });
        } else {
          entries.push({ id: String(Date.now()) + '-' + Math.random().toString(36).slice(2,7),
            number: roomEditor.draft.number, name: roomEditor.draft.name || '', x:x, y:y, w:width, h:height });
        }
        all[key] = entries;
        var saved = writeManualRooms(all);
        roomEditor.placing = false; roomEditor.start = null; roomEditor.editingId = null; roomEditor.movingId = null; roomEditor.draft = null;
        renderFloorPlan(activeFloorBuilding, activeFloorNumber);
        setRoomEditorStatus(saved ? 'Кабинет сохранён на этом устройстве. Нажмите на его метку, чтобы изменить или удалить.' : 'Не удалось сохранить кабинет: хранилище браузера недоступно.');
      });
      floorContent.addEventListener('pointercancel', function () {
        if (roomEditor.start && roomEditor.start.svg) {
          var preview = roomEditor.start.svg.querySelector('.fp-room-preview'); if (preview) preview.remove();
        }
        roomEditor.start = null; roomEditor.placing = false;
        setRoomEditorStatus('Размещение отменено.');
      });
    }
    var floorPanel = $('floorplan');
    if (floorPanel) floorPanel.addEventListener('click', function (event) { if (event.target === floorPanel) closeFloorPlan(); });
    var infoButton = $('infoBtn');
    if (infoButton) infoButton.addEventListener('click', function (event) {
      event.preventDefault();
      event.stopPropagation();
      var box = $('info');
      box.hidden = !box.hidden;
    });
    setMode(DEFAULT_MAP_MODE);
    setBuildingMode(DEFAULT_BUILDINGS_3D);
    setOrbit(false);
  }

  function showStats(s) {
    var el = $('stats');
    if (!el) { return; }
    el.textContent = 'Зданий на карте: ' + s.buildings;
  }

  function whenStyleReady() {
    return new Promise(function (resolve) {
      var done = false;
      function fire() { if (!done) { done = true; resolve(); } }
      if (map.isStyleLoaded()) { fire(); return; }
      map.once('load', fire);
      map.once('idle', fire);
      map.once('styledata', function () { if (map.isStyleLoaded()) { fire(); } });
      setTimeout(fire, 8000);
    });
  }

  var dataReady = Promise.all([
    loadJSON('campus-park-scene.json'),
    loadJSON('campus.json').catch(function () { return null; })
  ]);

  Promise.all([dataReady, whenStyleReady()]).then(function (res) {
    var data = res[0];
    setStatus('Строю 3D-сцену…');
    applyAtmosphere();
    wire();
    campusData = data[1];
    scene = G.buildScene(data[0], campusData, { renderTrees: false, renderWindows: false, renderShadows: false });
    syncSceneData();
    buildCampusMarkers();
    showStats(scene.stats);
    setStatus('');
    if (campusData && campusData.map && campusData.map.center) { HOME.center = campusData.map.center; }
    setTimeout(function () {
      var drawn = 0;
      try { drawn = map.queryRenderedFeatures({ layers: buildings3D ? ['bld', 'bld-footprint'] : ['campus-bld-2d'] }).length; } catch (e) { drawn = -1; }
      if (drawn === 0 && scene.stats.buildings) {
        setStatus('Здания не отрисовались. Если страница открыта как файл (content:// или file://), браузер может блокировать Web Worker. Откройте её с сайта проекта.');
        setTimeout(function () { setStatus(''); }, 15000);
      }
    }, 6000);
  }).catch(function (err) {
    var text = 'Не удалось построить карту: ' + ((err && err.message) || err) + '.';
    if (fetchLog.length) { text += ' Попытки: ' + fetchLog.join('; ') + '.'; }
    text += ' Если данные не грузятся, откройте страницу с сайта проекта или через локальный сервер.';
    setStatus(text, true);
  });
})();