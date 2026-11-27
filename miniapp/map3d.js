/* map3d.js — interactive 3D campus map. MapLibre GL (vendored) + local OSM scene.
 * No API keys. The only external requests are the optional Esri satellite tiles. */
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
  var mode = params.get('mode') === 'scheme' ? 'scheme' : 'sat';
  var HOME = { center: [30.3399, 59.99455], zoom: 16.6, pitch: 60, bearing: -28 };
  var EMPTY = { type: 'FeatureCollection', features: [] };
  var GROUND_LAYERS = ['bg', 'park', 'green', 'water', 'path-casing', 'path'];

  var ESRI_TILES = 'https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}';

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
      { id: 'sat', type: 'raster', source: 'esri', paint: { 'raster-fade-duration': 0 }, layout: { visibility: 'none' } },
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
      { id: 'bld-footprint', type: 'fill', source: 'ground', filter: ['==', ['get', 'kind'], 'building-footprint'],
        paint: { 'fill-color': ['get', 'col'], 'fill-opacity': 0.94, 'fill-outline-color': '#625e55' } },
      { id: 'shadow', type: 'fill', source: 'shadow',
        paint: { 'fill-color': '#16231a', 'fill-opacity': ['match', ['get', 'k'], 't', 0.24, 0.42] } },
      extrusion('bld', 'solid', 0, 1),
      extrusion('winframe', 'winframe', 16.4, 1),
      extrusion('win', 'win', 16.4, 1),
      extrusion('tree', 'tree', 0, 0.96),
      extrusion('hl', 'hl', 0, 0.55)
    ];
    return {
      version: 8,
      sources: {
        esri: {
          type: 'raster', tiles: [ESRI_TILES], tileSize: 256, maxzoom: 19,
          attribution: 'Imagery © Esri, Maxar, Earthstar Geographics, GIS User Community'
        },
        ground: { type: 'geojson', data: EMPTY },
        shadow: { type: 'geojson', data: EMPTY },
        solid: { type: 'geojson', data: EMPTY, maxzoom: 20, tolerance: 0 },
        winframe: { type: 'geojson', data: EMPTY, maxzoom: 20, tolerance: 0 },
        win: { type: 'geojson', data: EMPTY, maxzoom: 20, tolerance: 0 },
        tree: { type: 'geojson', data: EMPTY, maxzoom: 20, tolerance: 0 },
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
  map.addControl(new maplibregl.AttributionControl({ compact: true, customAttribution: '© OpenStreetMap contributors (ODbL) · Imagery © Esri, Maxar, Earthstar Geographics, GIS User Community' }), 'bottom-right');
  map.addControl(new maplibregl.NavigationControl({ visualizePitch: true }), 'top-right');
  map.on('error', function (ev) { if (ev && ev.error && window.console) { console.warn('map error:', ev.error.message || ev.error); } });

  var scene = null;
  var campusData = null;
  var popup = null;

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

  function setMode(next) {
    mode = next;
    var sat = mode === 'sat';
    // Keep geographic context in both modes. The schematic mode overlays
    // the campus paths/green areas on a subdued satellite base instead of
    // leaving the user with an isolated green polygon on a blank canvas.
    map.setLayoutProperty('sat', 'visibility', 'visible');
    map.setPaintProperty('sat', 'raster-opacity', sat ? 1 : 0.28);
    GROUND_LAYERS.forEach(function (id) { map.setLayoutProperty(id, 'visibility', sat ? 'none' : 'visible'); });
    var btn = $('btn-mode');
    if (btn) { btn.textContent = sat ? 'Схема' : 'Спутник'; btn.title = sat ? 'Показать схему вместо спутника' : 'Показать спутниковую подложку'; }
  }

  var layerStates = { shadow: true, tree: true, win: true };
  var layerLabels = { shadow: 'Тени', tree: 'Деревья', win: 'Окна' };

  function syncLayerButton(id, btnId) {
    var btn = $(btnId);
    if (!btn) return;
    var enabled = layerStates[id];
    btn.setAttribute('aria-pressed', enabled ? 'true' : 'false');
    btn.textContent = layerLabels[id] + (enabled ? ': Вкл' : ': Выкл');
    btn.title = layerLabels[id] + (enabled ? ' включены' : ' выключены');
  }

  function toggleLayer(id, btnId) {
    layerStates[id] = !layerStates[id];
    map.setLayoutProperty(id, 'visibility', layerStates[id] ? 'visible' : 'none');
    syncLayerButton(id, btnId);
    // Repaint explicitly so the change is immediately visible on mobile WebViews.
    map.triggerRepaint();
  }

  /* ---------- popup / selection ---------- */

  function popupHtml(p) {
    var camp = null;
    ((campusData && campusData.buildings) || []).forEach(function (b) {
      if (String(b.number) === String(p.campus) || (b.osm && String(b.osm.id) === String(p.pid))) { camp = b; }
    });
    var html = '<div class="pop">';
    var title = camp ? camp.name : (p.name || 'Здание');
    var heritage = camp && camp.heritage;
    var photo = heritage && heritage.photo;
    if (photo && photo.src) {
      var photoUrl = null;
      try { photoUrl = new URL(photo.src, location.href).href; } catch (e) { photoUrl = null; }
      if (photoUrl && /^https?:/.test(photoUrl)) {
        html += '<figure class="campus-photo"><img src="' + esc(photoUrl) + '" alt="' + esc(photo.alt || title) + '" loading="lazy" decoding="async">';
        html += '<figcaption>' + esc(photo.credit || 'Фото Wikimedia Commons');
        if (photo.sourceUrl && /^https:\/\/commons\.wikimedia\.org\//.test(photo.sourceUrl)) html += ' · <a href="' + esc(photo.sourceUrl) + '" target="_blank" rel="noopener noreferrer">источник</a>';
        if (photo.licenseUrl && /^https:\/\/creativecommons\.org\//.test(photo.licenseUrl)) html += ' · <a href="' + esc(photo.licenseUrl) + '" target="_blank" rel="noopener noreferrer">' + esc(photo.license || 'лицензия') + '</a>';
        html += '</figcaption></figure>';
      }
    }
    html += '<h3>' + esc(title) + '</h3>';
    if (camp && camp.description) { html += '<p>' + esc(camp.description) + '</p>'; }
    if (camp && camp.address) { html += '<p class="muted">' + esc(camp.address) + '</p>'; }
    if (p.hq === 'known') { html += '<p><b>Высота:</b> ' + esc(p.hm) + ' м (указана в OSM)</p>'; }
    else if (p.hq === 'levels') { html += '<p><b>Высота:</b> около ' + esc(p.hm) + ' м (оценка по этажности OSM)</p>'; }
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

  function selectBuilding(pid, lngLat) {
    var info = scene && scene.index[pid];
    if (!info) { return; }
    if (popup) { var old = popup; popup = null; old.remove(); }
    map.getSource('hl').setData({ type: 'FeatureCollection', features: scene.highlight[pid] || [] });
    var mine = new maplibregl.Popup({ closeButton: true, maxWidth: '320px', className: 'bpop', focusAfterOpen: false })
      .setLngLat(lngLat).setHTML(popupHtml(info)).addTo(map);
    popup = mine;
    mine.on('close', function () { if (popup === mine) { popup = null; map.getSource('hl').setData(EMPTY); } });
  }

  map.on('click', function (e) {
    var hits = map.queryRenderedFeatures(e.point, { layers: ['bld', 'win', 'bld-footprint'] });
    if (!hits.length) { clearSelection(); return; }
    selectBuilding(String(hits[0].properties.pid), e.lngLat);
  });
  map.on('mousemove', function (e) {
    var hits = map.queryRenderedFeatures(e.point, { layers: ['bld', 'win'] });
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
          case 'btn-shadow': toggleLayer('shadow', 'btn-shadow'); break;
          case 'btn-tree': toggleLayer('tree', 'btn-tree'); break;
          case 'btn-win': toggleLayer('win', 'btn-win'); break;
          case 'btn-orbit': setOrbit(!orbiting); break;
          case 'btn-home':
            setOrbit(false);
            map.flyTo({ center: HOME.center, zoom: HOME.zoom, pitch: HOME.pitch, bearing: HOME.bearing, duration: 1800 });
            break;
        }
      });
    }
    var infoButton = $('infoBtn');
    if (infoButton) infoButton.addEventListener('click', function (event) {
      event.preventDefault();
      event.stopPropagation();
      var box = $('info');
      box.hidden = !box.hidden;
    });
    ['shadow', 'tree', 'win'].forEach(function (id) {
      map.setLayoutProperty(id, 'visibility', 'visible');
      syncLayerButton(id, id === 'shadow' ? 'btn-shadow' : id === 'tree' ? 'btn-tree' : 'btn-win');
    });
    setOrbit(false);
  }

  function buildChips() {
    var host = $('chips');
    var list = ((campusData && campusData.buildings) || []).slice().sort(function (a, b) { return parseInt(a.number, 10) - parseInt(b.number, 10); });
    host.innerHTML = '';
    list.forEach(function (b) {
      var btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'chip';
      btn.textContent = 'Корпус ' + b.number;
      btn.addEventListener('click', function () {
        setOrbit(false);
        map.flyTo({ center: [b.longitude, b.latitude], zoom: 18.4, pitch: 64, bearing: map.getBearing(), duration: 1600 });
        if (b.osm && scene && scene.index[String(b.osm.id)]) {
          setTimeout(function () { selectBuilding(String(b.osm.id), [b.longitude, b.latitude]); }, 1700);
        }
      });
      host.appendChild(btn);
    });
  }

  function showStats(s) {
    var el = $('stats');
    if (!el) { return; }
    el.textContent = 'Зданий на карте: ' + s.buildings + ' · деревьев: ' + s.trees;
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
    scene = G.buildScene(data[0], campusData);
    map.getSource('ground').setData({ type: 'FeatureCollection', features: scene.ground });
    map.getSource('shadow').setData({ type: 'FeatureCollection', features: scene.shadows });
    map.getSource('solid').setData({ type: 'FeatureCollection', features: scene.solid });
    map.getSource('winframe').setData({ type: 'FeatureCollection', features: scene.windowFrames });
    map.getSource('win').setData({ type: 'FeatureCollection', features: scene.windows });
    map.getSource('tree').setData({ type: 'FeatureCollection', features: scene.trees });
    setMode(mode);
    buildChips();
    showStats(scene.stats);
    setStatus('');
    if (campusData && campusData.map && campusData.map.center) { HOME.center = campusData.map.center; }
    setTimeout(function () {
      var drawn = 0;
      try { drawn = map.queryRenderedFeatures({ layers: ['bld'] }).length; } catch (e) { drawn = -1; }
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