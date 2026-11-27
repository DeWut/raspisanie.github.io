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
    var match = value.match(/^([1-9])[- ]?\\d{2,3}$/);
    return match ? Number(match[1]) : null;
  }

  function openFloorPlan(number) {
    var building = ((campusData && campusData.buildings) || []).find(function (b) { return String(b.number) === String(number); });
    if (!building) return;
    var panel = $('floorplan');
    var title = $('floorplan-title');
    var content = $('floorplan-content');
    if (!panel || !content) return;
    title.textContent = building.name + ' · ' + (building.address || '');
    var floors = {};
    var unassigned = [];
    (building.units || []).forEach(function (unit) {
      var floor = floorForRoom(unit.room);
      if (floor == null) unassigned.push(unit);
      else (floors[floor] || (floors[floor] = [])).push(unit);
    });
    var keys = Object.keys(floors).sort(function (a, b) { return Number(a) - Number(b); });
    var html = '';
    if (keys.length) {
      keys.forEach(function (floor) {
        html += '<section class="floor-section"><h4>' + floor + '-й этаж</h4><div class="room-grid">';
        floors[floor].forEach(function (unit) {
          html += '<article class="room-card"><b>' + esc(unit.room) + '</b><span>' + esc(unit.name) + '</span></article>';
        });
        html += '</div></section>';
      });
    }
    if (unassigned.length) {
      html += '<section class="floor-section"><h4>Этаж не указан в данных</h4><div class="room-grid">';
      unassigned.forEach(function (unit) {
        html += '<article class="room-card"><b>' + esc(unit.room || '—') + '</b><span>' + esc(unit.name) + '</span></article>';
      });
      html += '</div></section>';
    }
    html += '<p class="floor-note">Это справочная схема размещения подразделений по номерам аудиторий, а не точный архитектурный план: координаты кабинетов и расположение лестниц пока не подтверждены.</p>';
    content.innerHTML = html || '<p>Для этого корпуса пока нет опубликованных данных о кабинетах.</p>';
    panel.hidden = false;
    panel.setAttribute('aria-hidden', 'false');
  }

  function closeFloorPlan() {
    var panel = $('floorplan');
    if (panel) { panel.hidden = true; panel.setAttribute('aria-hidden', 'true'); }
  }

  function selectBuilding(pid, lngLat) {
    var info = scene && scene.index[pid];
    if (!info) { return; }
    if (popup) { var old = popup; popup = null; old.remove(); }
    map.getSource('hl').setData({ type: 'FeatureCollection', features: scene.highlight[pid] || [] });
    var mine = new maplibregl.Popup({ closeButton: true, maxWidth: '320px', className: 'bpop', focusAfterOpen: false })
      .setLngLat(lngLat).setHTML(popupHtml(info)).addTo(map);
    popup = mine;
    var floorButton = mine.getElement().querySelector('[data-open-floorplan]');
    if (floorButton) floorButton.addEventListener('click', function () { openFloorPlan(floorButton.getAttribute('data-open-floorplan')); });
    mine.on('close', function () { if (popup === mine) { popup = null; map.getSource('hl').setData(EMPTY); } });
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
        map.flyTo({ center: [b.longitude, b.latitude], zoom: 18.4, pitch: mode === 'scheme' && !buildings3D ? 0 : 64, bearing: mode === 'scheme' ? 0 : map.getBearing(), duration: 1600 });
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
    buildChips();
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