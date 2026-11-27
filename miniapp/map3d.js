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
    setStatus('Не удалось загрузить карту.', true);
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
  var buildings3D = true;
  var DEFAULT_MAP_MODE = 'scheme';
  var DEFAULT_BUILDINGS_3D = true;
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
    setStatus('3D-режим недоступен: ' + e.message, true);
    return;
  }

  // Recalculate the map viewport after its parent navigation dialog becomes visible.
  window.addEventListener('resize', function () {
    if (map) map.resize();
  });

  // 3D is permanent. Use mapped height geometry where available; retain flat
  // footprints only for buildings without reliable height data.
  function setBuildingMode() {
    buildings3D = true;
    var hasExtrusions = !!(scene && scene.solid && scene.solid.length);
    var showExtrusions = hasExtrusions;
    if (map && map.getLayer('bld')) map.setLayoutProperty('bld', 'visibility', showExtrusions ? 'visible' : 'none');
    if (map && map.getLayer('campus-bld-2d')) map.setLayoutProperty('campus-bld-2d', 'visibility', showExtrusions ? 'none' : 'visible');
    if (map && map.getLayer('bld-footprint')) map.setLayoutProperty('bld-footprint', 'visibility', 'visible');
    if (map && map.getLayer('hl')) map.setLayoutProperty('hl', 'visibility', 'visible');
    if (map && map.getLayer('hl-flat')) map.setLayoutProperty('hl-flat', 'visibility', 'none');
    if (map) map.easeTo({ pitch: 58, bearing: -18, duration: 650 });
  }
  map.on('error', function (ev) { if (ev && ev.error && window.console) { console.warn('map error:', ev.error.message || ev.error); } });
  map.on('style.load', function () { if (scene) syncSceneData(); setBuildingMode(buildings3D); });

  function updateModeButton() {
    var button = $('btn-mode');
    var label = $('map-mode-label');
    if (button) {
      if (label) label.textContent = mode === 'sat' ? 'Спутник' : 'Схема';
      button.title = mode === 'sat' ? 'Сейчас спутник. Нажмите, чтобы включить схему' : 'Сейчас схема. Нажмите, чтобы включить спутник';
      button.setAttribute('aria-label', mode === 'sat' ? 'Переключить на схему' : 'Переключить на спутник');
      button.setAttribute('aria-pressed', mode === 'sat' ? 'true' : 'false');
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
      setStatus('Загрузка карты…');
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
        map.flyTo({ center: coordinate, zoom: 18.4, pitch: 58, bearing: map.getBearing(), duration: 900 });
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

  function commonsCategoryTitle(sourceUrl) {
    try {
      var url = new URL(sourceUrl);
      if (url.hostname !== 'commons.wikimedia.org') return '';
      var match = url.pathname.match(/^\/wiki\/Category:(.+)$/);
      return match ? decodeURIComponent(match[1]).replace(/_/g, ' ') : '';
    } catch (e) { return ''; }
  }

  function wikiPlainText(value) {
    if (!value) return '';
    var node = document.createElement('div');
    node.innerHTML = String(value);
    return (node.textContent || '').replace(/\s+/g, ' ').trim();
  }

  function loadCommonsGallery(container, building) {
    if (!container || !building || !building.heritage) return;
    if (String(building.number) === '1' || building.mapKey === 'kor_gl' || building.heritage.heritageId === '7810193007') return;
    var gallery = container.querySelector('[data-commons-gallery]');
    if (!gallery) return;
    var heritage = building.heritage;
    var category = commonsCategoryTitle(heritage.heritageSourceUrl);
    if (!category) return;

    var params = new URLSearchParams({
      action: 'query',
      generator: 'categorymembers',
      gcmtitle: 'Category:' + category,
      gcmtype: 'file',
      gcmnamespace: '6',
      gcmlimit: '8',
      gcmsort: 'timestamp',
      gcmdir: 'desc',
      prop: 'imageinfo',
      iiprop: 'url|extmetadata|user',
      iiurlwidth: '640',
      format: 'json',
      origin: '*'
    });

    fetch('https://commons.wikimedia.org/w/api.php?' + params.toString())
      .then(function (response) {
        if (!response.ok) throw new Error('Commons HTTP ' + response.status);
        return response.json();
      })
      .then(function (data) {
        if (!gallery.isConnected) return;
        var pages = data.query && data.query.pages ? Object.keys(data.query.pages).map(function (key) { return data.query.pages[key]; }) : [];
        var images = pages.map(function (page) {
          var info = page.imageinfo && page.imageinfo[0];
          if (!info || !info.thumburl || !/^https:\/\//.test(info.thumburl)) return null;
          if (!/\.(jpe?g|png|webp)$/i.test(page.title || '')) return null;
          var meta = info.extmetadata || {};
          var author = wikiPlainText(meta.Artist && meta.Artist.value) || info.user || '';
          var license = wikiPlainText(meta.LicenseShortName && meta.LicenseShortName.value) || '';
          return {
            title: page.title || 'Фотография корпуса',
            thumb: info.thumburl,
            page: info.descriptionurl || ('https://commons.wikimedia.org/wiki/' + encodeURIComponent(page.title || '').replace(/%20/g, '_')),
            author: author,
            license: license,
            licenseUrl: wikiPlainText(meta.LicenseUrl && meta.LicenseUrl.value)
          };
        }).filter(Boolean).slice(0, 6);

        if (!images.length) return;
        var strip = document.createElement('div');
        strip.className = 'commons-strip';
        images.forEach(function (item) {
          var card = document.createElement('div');
          card.className = 'commons-item';
          var link = document.createElement('a');
          link.className = 'commons-image-link';
          link.href = item.page;
          link.target = '_blank';
          link.rel = 'noopener noreferrer';
          link.setAttribute('aria-label', 'Открыть фотографию на Wikimedia Commons');
          var image = document.createElement('img');
          image.src = item.thumb;
          image.alt = item.title.replace(/^File:/, '');
          image.loading = 'lazy';
          image.decoding = 'async';
          image.onerror = function () { card.remove(); };
          link.appendChild(image);
          card.appendChild(link);
          var caption = document.createElement('span');
          caption.className = 'commons-caption';
          caption.textContent = item.author || 'Wikimedia Commons';
          card.appendChild(caption);
          if (item.license && item.licenseUrl && /^https:\/\//.test(item.licenseUrl)) {
            var licenseLink = document.createElement('a');
            licenseLink.className = 'commons-license';
            licenseLink.href = item.licenseUrl;
            licenseLink.target = '_blank';
            licenseLink.rel = 'noopener noreferrer';
            licenseLink.textContent = item.license;
            card.appendChild(licenseLink);
          } else if (item.license) {
            var licenseText = document.createElement('span');
            licenseText.className = 'commons-caption';
            licenseText.textContent = item.license;
            card.appendChild(licenseText);
          }
          strip.appendChild(card);
        });
        if (strip.children.length) {
          gallery.innerHTML = '';
          gallery.appendChild(strip);
        }
      })
      .catch(function () {
        /* Оставляем локальную фотографию, если Commons временно недоступен. */
      });
  }

  function popupHtml(p) {
    var camp = null;
    ((campusData && campusData.buildings) || []).forEach(function (b) {
      if (String(b.number) === String(p.campus) || (b.osm && (String(b.osm.id) === String(p.pid) || String(b.osm.outerWayId || '') === String(p.pid)))) { camp = b; }
    });
    var html = '<div class="pop">';
    var title = camp ? camp.name : (p.name || 'Здание');
    var heritage = camp && camp.heritage;
    var photo = heritage && heritage.photo;

    html += '<h3>' + esc(title) + '</h3>';
    if (camp && camp.description) html += '<p>' + esc(camp.description) + '</p>';
    if (camp && camp.address) html += '<p class="muted">' + esc(camp.address) + '</p>';

    if (heritage) {
      if (heritage.historicalNote) html += '<section class="building-history"><b>История</b><p>' + esc(heritage.historicalNote) + '</p></section>';
      if (heritage.built) html += '<p><b>Построен:</b> ' + esc(heritage.built) + '</p>';
      if (heritage.style) html += '<p><b>Стиль:</b> ' + esc(heritage.style) + '</p>';
      if (heritage.architects) html += '<p><b>Архитекторы:</b> ' + esc(heritage.architects) + '</p>';

      if (heritage.heritageSourceUrl && /^https:\/\/commons\.wikimedia\.org\//.test(heritage.heritageSourceUrl)) {
        html += '<section class="wiki-card commons-card"><b>' + (String(camp.number) === '1' ? 'Фотография главного корпуса' : 'Фотографии корпуса') + '</b><div data-commons-gallery>';
        if (photo && photo.src) {
          var photoUrl = null;
          try { photoUrl = new URL(photo.src, location.href).href; } catch (e) { photoUrl = null; }
          if (photoUrl && /^https?:/.test(photoUrl)) {
            html += '<figure class="campus-photo"><a class="campus-photo-link" href="' + esc(photo.sourceUrl || heritage.heritageSourceUrl) + '" target="_blank" rel="noopener noreferrer"><img src="' + esc(photoUrl) + '" alt="' + esc(photo.alt || title) + '" loading="lazy" decoding="async" data-campus-fallback-photo></a><figcaption>' + esc(photo.credit || 'Wikimedia Commons');
            if (photo.licenseUrl && /^https:\/\/creativecommons\.org\//.test(photo.licenseUrl)) html += ' · <a href="' + esc(photo.licenseUrl) + '" target="_blank" rel="noopener noreferrer">' + esc(photo.license || 'лицензия') + '</a>';
            html += '</figcaption></figure>';
          }
        }
        html += '</div><a class="commons-more" href="' + esc(heritage.heritageSourceUrl) + '" target="_blank" rel="noopener noreferrer">Открыть Wikimedia Commons</a></section>';
      }
      if (heritage.historySourceUrl && /^https?:\/\//.test(heritage.historySourceUrl)) {
        html += '<p><a href="' + esc(heritage.historySourceUrl) + '" target="_blank" rel="noopener noreferrer">Подробнее об истории</a></p>';
      }
    }

    if (camp && camp.units && camp.units.length) {
      html += '<section class="building-units"><b>Подразделения и кабинеты</b><ul>';
      camp.units.forEach(function (u) {
        html += '<li>' + esc(u.name) + (u.room ? ' <span class="muted">— ' + esc(u.room) + '</span>' : '') + '</li>';
      });
      html += '</ul></section>';
    }
    if (camp && ['1', '2'].indexOf(String(camp.number)) !== -1) {
      html += '<p><button type="button" class="floor-open" data-open-floorplan="' + esc(camp.number) + '">Планы этажей</button></p>';
    }
    if (camp && camp.officialSource && /^https?:\/\//.test(camp.officialSource)) {
      html += '<p class="muted"><a href="' + esc(camp.officialSource) + '" target="_blank" rel="noopener noreferrer">На официальной карте университета</a></p>';
    }
    if (p.src && /^https:\/\/www\.openstreetmap\.org\//.test(p.src)) {
      html += '<p class="muted"><a href="' + esc(p.src) + '" target="_blank" rel="noopener">Объект на OpenStreetMap</a></p>';
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
    var roomNumber = value.match(/(?:^|[^\d])([1-9])\d{2,3}[А-Яа-яA-Za-z]?(?:$|[^\d])/);
    if (roomNumber) return Number(roomNumber[1]);
    var shortRoom = value.match(/^\s*\d{1,2}[А-Яа-яA-Za-z]?\s*$/);
    return shortRoom ? 1 : null;
  }

  function floorForUnit(unit) {
    return unit && Number(unit.floor) > 0 ? Number(unit.floor) : floorForRoom(unit && unit.room);
  }

  function normalizeRoom(value) {
    return String(value == null ? '' : value).toUpperCase().replace(/Ё/g, 'Е')
      .replace(/АУДИТОРИЯ|КАБИНЕТ|КАБ\.?|№|Nº|NO\.?/g, '')
      .replace(/[^0-9A-ZА-Я]/g, '');
  }

  function isNumberedRoom(value) {
    return /^\s*\d{1,4}[А-Яа-яA-Za-z]?(?:[-/]\d+)?(?:\s*,\s*\d{1,4}[А-Яа-яA-Za-z]?)?\s*$/.test(String(value || '').trim());
  }

  function roomUnit(building, room) {
    var key = normalizeRoom(room);
    return ((building && building.units) || []).find(function (unit) {
      return normalizeRoom(unit.room) === key;
    }) || null;
  }

  var activeFloorBuilding = null;
  var activeFloorNumber = 1;
  var floorPlanZoom = 1;
  var floorPlanFitWidth = 100;
  var selectedFloorRoom = '';

  function updateFloorRoute(building, floor, room) {
    if (!building || !window.history || !window.URLSearchParams) return;
    var params = new URLSearchParams(location.search);
    var mapKey = building.mapKey || ('kor_' + String(building.number));
    params.set('auditory', mapKey);
    params.set('floor', String(floor || 1));
    if (room) params.set('room', String(room));
    else params.delete('room');
    var query = params.toString();
    history.replaceState(history.state, '', location.pathname + (query ? '?' + query : '') + location.hash);
  }

  function clearFloorRoute() {
    if (!window.history || !window.URLSearchParams) return;
    var params = new URLSearchParams(location.search);
    ['auditory', 'building', 'campus', 'floor', 'room'].forEach(function (key) { params.delete(key); });
    var query = params.toString();
    history.replaceState(history.state, '', location.pathname + (query ? '?' + query : '') + location.hash);
  }

  function selectRoomInPlan(room) {
    if (!activeFloorBuilding || !room) return;
    selectedFloorRoom = String(room);
    updateFloorRoute(activeFloorBuilding, activeFloorNumber, selectedFloorRoom);
    var content = $('floorplan-content');
    if (!content) return;
    content.querySelectorAll('[data-room].is-selected').forEach(function (node) { node.classList.remove('is-selected'); });
    content.querySelectorAll('.room-card.is-selected').forEach(function (node) { node.classList.remove('is-selected'); });
    var target = null;
    content.querySelectorAll('[data-room]').forEach(function (node) {
      if (normalizeRoom(node.getAttribute('data-room')) === normalizeRoom(room)) target = node;
    });
    content.querySelectorAll('[data-room-card]').forEach(function (node) {
      if (normalizeRoom(node.getAttribute('data-room-card')) === normalizeRoom(room)) {
        node.classList.add('is-selected');
        if (!target) target = node;
      }
    });
    var detail = $('room-selection');
    var unit = roomUnit(activeFloorBuilding, room);
    if (detail) {
      detail.hidden = false;
      detail.innerHTML = '';
      var heading = document.createElement('b');
      heading.textContent = unit
        ? (isNumberedRoom(unit.room) ? 'Аудитория ' + unit.room : (unit.room || unit.name))
        : ('Аудитория ' + room);
      var description = document.createElement('span');
      description.textContent = unit ? unit.name : 'Помещение на плане';
      var locationNote = document.createElement('small');
      locationNote.textContent = target && target.matches('[data-room]')
        ? 'Место аудитории отмечено на схеме.'
        : 'Аудитория есть в справочнике, но её точное место на схеме пока не привязано.';
      detail.appendChild(heading);
      detail.appendChild(description);
      detail.appendChild(locationNote);
    }
    if (target && target.matches('[data-room]')) {
      target.classList.add('is-selected');
      target.scrollIntoView({ behavior: 'smooth', block: 'center', inline: 'center' });
    } else if (target) {
      target.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }
  }

  function fitFloorPlanDrawing() {
    var content = $('floorplan-content');
    var viewport = content && content.querySelector('.floorplan-viewport');
    var drawing = content && content.querySelector('.floorplan-drawing');
    var svg = drawing && drawing.querySelector('.floorplan-svg');
    if (!viewport || !drawing || !svg || !viewport.clientWidth || !viewport.clientHeight) return;
    var values = (svg.getAttribute('viewBox') || '0 0 1000 760').trim().split(/[ ,]+/).map(Number);
    var width = values[2], height = values[3];
    if (!(width > 0 && height > 0)) return;
    var widthForFullHeight = viewport.clientHeight * width / height;
    floorPlanFitWidth = Math.min(100, widthForFullHeight / viewport.clientWidth * 100);
    drawing.style.width = (floorPlanFitWidth * floorPlanZoom) + '%';
  }

  function renderFloorPlan(building, floor, selectedRoom) {
    var content = $('floorplan-content');
    if (!content) return;
    activeFloorBuilding = building;
    activeFloorNumber = Number(floor) || 1;
    selectedFloorRoom = selectedRoom || '';
    floorPlanZoom = 1;
    var custom = window.CAMPUS_FLOORPLANS && window.CAMPUS_FLOORPLANS[String(building.number)];
    var units = building.units || [];
    var html = '';

    if (custom && custom.floors && custom.floors[activeFloorNumber]) {
      var plan = custom.floors[activeFloorNumber];
      var tabs = '';
      Object.keys(custom.floors).sort(function (a,b) { return Number(a)-Number(b); }).forEach(function (key) {
        tabs += '<button type="button" class="floor-tab' + (Number(key) === activeFloorNumber ? ' active' : '') + '" data-floor-select="' + key + '" aria-pressed="' + (Number(key) === activeFloorNumber ? 'true' : 'false') + '">' + key + ' этаж</button>';
      });
      var assigned = units.filter(function (unit) { return floorForUnit(unit) === activeFloorNumber; });
      html += '<nav class="floor-tabs" aria-label="Выбор этажа">' + tabs + '</nav>';
      html += '<div class="floor-toolbar"><span>' + esc(plan.title) + '</span><div><button type="button" data-floor-zoom="-1" aria-label="Уменьшить схему">−</button><output data-floor-zoom-value aria-live="polite">100%</output><button type="button" data-floor-zoom="1" aria-label="Увеличить схему">+</button><button type="button" data-floor-zoom="reset">Вписать</button></div></div>';
      html += '<div class="floorplan-viewport" aria-label="План этажа"><div class="floorplan-drawing" style="width:100%">' + plan.svg + '</div></div>';
      html += '<div id="room-selection" class="room-selection" aria-live="polite" hidden></div>';
      if (assigned.length) html += '<section class="floor-units"><h4>Кабинеты этого этажа</h4>';
      if (assigned.length) {
        html += '<div class="room-grid">';
        assigned.forEach(function (unit) {
          html += '<button type="button" class="room-card" data-room-card="' + esc(unit.room || '') + '" data-room-select="' + esc(unit.room || '') + '"><b>' + esc(unit.room || 'Этаж ' + activeFloorNumber) + '</b><span>' + esc(unit.name) + '</span></button>';
        });
        html += '</div>';
      }
      if (assigned.length) html += '</section>';
      html += '<p class="floor-note">Схема по предоставленному плану; размеры условные.</p>';
      content.innerHTML = html;
      fitFloorPlanDrawing();
      if (selectedFloorRoom) selectRoomInPlan(selectedFloorRoom);
      return;
    }

    var floors = {};
    var unassigned = [];
    units.forEach(function (unit) {
      var f = floorForUnit(unit);
      if (f == null) unassigned.push(unit);
      else (floors[f] || (floors[f] = [])).push(unit);
    });
    var keys = Object.keys(floors).sort(function (a, b) { return Number(a) - Number(b); });
    if (keys.length) {
      html += '<nav class="floor-tabs" aria-label="Выбор этажа">';
      keys.forEach(function (key) {
        html += '<button type="button" class="floor-tab' + (Number(key) === activeFloorNumber ? ' active' : '') + '" data-floor-select="' + key + '" aria-pressed="' + (Number(key) === activeFloorNumber ? 'true' : 'false') + '">' + key + ' этаж</button>';
      });
      html += '</nav>';
    }
    html += '<div class="floor-toolbar"><span>' + (keys.length ? activeFloorNumber + '-й этаж' : 'Справочник помещений') + '</span></div>';
    var visibleUnits = floors[activeFloorNumber] || [];
    if (visibleUnits.length) {
      html += '<section class="floor-units"><h4>Аудитории и подразделения</h4><div class="room-grid">';
      visibleUnits.forEach(function (unit) {
        html += '<button type="button" class="room-card" data-room-card="' + esc(unit.room || '') + '" data-room-select="' + esc(unit.room || '') + '"><b>' + esc(unit.room || 'Без номера') + '</b><span>' + esc(unit.name) + '</span></button>';
      });
      html += '</div></section>';
    } else {
      html += '<p class="muted">Для этого этажа пока нет помещений в справочнике.</p>';
    }
    if (unassigned.length) {
      html += '<section class="floor-section"><h4>Этаж не указан</h4><div class="room-grid">';
      unassigned.forEach(function (unit) {
        html += '<button type="button" class="room-card" data-room-card="' + esc(unit.room || '') + '" data-room-select="' + esc(unit.room || '') + '"><b>' + esc(unit.room || '—') + '</b><span>' + esc(unit.name) + '</span></button>';
      });
      html += '</div></section>';
    }
    html += '<p class="floor-note">Графический план этого корпуса пока не подготовлен. Здесь показан справочник помещений, а не точное расположение кабинетов.</p>';
    content.innerHTML = html;
    if (selectedFloorRoom) selectRoomInPlan(selectedFloorRoom);
  }

  function openFloorPlan(number, requestedFloor, requestedRoom, fromRoute) {
    var building = ((campusData && campusData.buildings) || []).find(function (b) { return String(b.number) === String(number); });
    if (!building) return;
    var panel = $('floorplan');
    var title = $('floorplan-title');
    if (!panel) return;
    title.textContent = building.name + ' · ' + (building.address || '');
    var custom = window.CAMPUS_FLOORPLANS && window.CAMPUS_FLOORPLANS[String(building.number)];
    var availableFloors = custom && custom.floors ? Object.keys(custom.floors).map(Number) : [];
    (building.units || []).forEach(function (unit) {
      var f = floorForUnit(unit);
      if (f && availableFloors.indexOf(f) === -1) availableFloors.push(f);
    });
    availableFloors.sort(function (a,b) { return a-b; });
    var firstFloor = availableFloors.length ? availableFloors[0] : 1;
    var floor = Number(requestedFloor) || firstFloor;
    if (availableFloors.length && availableFloors.indexOf(floor) === -1) floor = firstFloor;
    floorPlanSearchQuery = '';
    panel.hidden = false;
    panel.style.display = 'grid';
    panel.setAttribute('aria-hidden', 'false');
    renderFloorPlan(building, floor, requestedRoom || '');
    if (!window.CAMPUS_FLOORPLANS || !window.CAMPUS_FLOORPLANS[String(building.number)]) {
      setStatus('Для этого корпуса пока нет графического плана этажей.', true);
      setTimeout(function () { setStatus(''); }, 5000);
    }
    if (!fromRoute) updateFloorRoute(building, floor, requestedRoom);
  }

  function closeFloorPlan() {
    var panel = $('floorplan');
    if (panel) { panel.hidden = true; panel.style.display = 'none'; panel.setAttribute('aria-hidden', 'true'); }
    activeFloorBuilding = null;
    selectedFloorRoom = '';
    floorPlanSearchQuery = '';
    clearFloorRoute();
  }

  function openFloorPlanFromLocation() {
    var params = new URLSearchParams(location.search);
    var key = params.get('auditory') || params.get('building') || params.get('campus');
    if (!key) return;
    var buildings = (campusData && campusData.buildings) || [];
    var building = buildings.find(function (item) {
      return String(item.number) === String(key) || String(item.mapKey || '').toLowerCase() === String(key).toLowerCase();
    });
    var room = params.get('room') || '';
    if (!building && room) {
      building = buildings.find(function (item) { return !!roomUnit(item, room); });
    }
    if (!building && key) {
      building = buildings.find(function (item) { return !!roomUnit(item, key); });
      if (building && !room) room = key;
    }
    if (!building) return;
    var coordinate = [Number(building.longitude), Number(building.latitude)];
    if (isFinite(coordinate[0]) && isFinite(coordinate[1])) {
      map.flyTo({ center: coordinate, zoom: 18.2, pitch: 58, duration: 700 });
    }
    openFloorPlan(building.number, Number(params.get('floor')) || undefined, room, true);
  }

  function showBuildingPopup(info, lngLat) {
    if (!info) return;
    if (popup) { var old = popup; popup = null; old.remove(); }
    var highlight = info.pid && scene && scene.highlight[info.pid] ? scene.highlight[info.pid] : [];
    if (map.getSource('hl')) map.getSource('hl').setData({ type: 'FeatureCollection', features: highlight });
    var mine = new maplibregl.Popup({ closeButton: true, maxWidth: '340px', className: 'bpop', focusAfterOpen: false })
      .setLngLat(lngLat).setHTML(popupHtml(info)).addTo(map);
    popup = mine;
    var selectedCampus = ((campusData && campusData.buildings) || []).find(function (b) { return String(b.number) === String(info.campus); });
    loadCommonsGallery(mine.getElement(), selectedCampus);
    mine.getElement().querySelectorAll('[data-campus-fallback-photo]').forEach(function (image) {
      image.addEventListener('error', function () { var figure = image.closest('.campus-photo'); if (figure) figure.remove(); }, { once: true });
    });
    // Delegated at document level in wire(); keep popup markup independent of timing.
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
    document.addEventListener('click', function (event) {
      var floorButton = event.target.closest && event.target.closest('[data-open-floorplan]');
      if (!floorButton) return;
      event.preventDefault();
      event.stopPropagation();
      openFloorPlan(floorButton.getAttribute('data-open-floorplan'));
    }, true);
    var modeButton = $('btn-mode');
    if (modeButton) modeButton.addEventListener('click', function (event) {
      event.preventDefault();
      event.stopPropagation();
      setMode(mode === 'sat' ? 'scheme' : 'sat');
    });
    var floorClose = $('floorplan-close');
    if (floorClose) floorClose.addEventListener('click', closeFloorPlan);
    var floorContent = $('floorplan-content');
    if (floorContent) {
      floorContent.addEventListener('click', function (event) {
        var tab = event.target.closest('[data-floor-select]');
        if (tab && activeFloorBuilding) {
          floorPlanSearchQuery = '';
          renderFloorPlan(activeFloorBuilding, Number(tab.getAttribute('data-floor-select')), '');
          updateFloorRoute(activeFloorBuilding, activeFloorNumber, '');
          return;
        }
        var roomButton = event.target.closest('[data-room-select]');
        if (roomButton && activeFloorBuilding) {
          var room = roomButton.getAttribute('data-room-select');
          var targetFloor = Number(roomButton.getAttribute('data-room-floor')) || floorForRoom(room);
          if (targetFloor && targetFloor !== activeFloorNumber) {
            renderFloorPlan(activeFloorBuilding, targetFloor, room);
          } else {
            selectRoomInPlan(room);
          }
          return;
        }
        var roomShape = event.target.closest('[data-room]');
        if (roomShape && activeFloorBuilding) {
          selectRoomInPlan(roomShape.getAttribute('data-room'));
          return;
        }
        var zoom = event.target.closest('[data-floor-zoom]');
        if (zoom) {
          var action = zoom.getAttribute('data-floor-zoom');
          floorPlanZoom = action === 'reset' ? 1 : Math.max(1, Math.min(3, floorPlanZoom + Number(action) * 0.25));
          var drawing = floorContent.querySelector('.floorplan-drawing');
          if (drawing) drawing.style.width = (floorPlanFitWidth * floorPlanZoom) + '%';
          var zoomValue = floorContent.querySelector('[data-floor-zoom-value]');
          if (zoomValue) zoomValue.textContent = Math.round(floorPlanZoom * 100) + '%';
        }
      });
      floorContent.addEventListener('keydown', function (event) {
        var roomShape = event.target.closest && event.target.closest('[data-room]');
        if (roomShape && (event.key === 'Enter' || event.key === ' ')) {
          event.preventDefault();
          selectRoomInPlan(roomShape.getAttribute('data-room'));
        }
      });
    }

    window.addEventListener('resize', function () {
      var panel = $('floorplan');
      if (panel && !panel.hidden) fitFloorPlanDrawing();
    });
    var floorPanel = $('floorplan');
    if (floorPanel) floorPanel.addEventListener('click', function (event) { if (event.target === floorPanel) closeFloorPlan(); });
    if (!window.CAMPUS_FLOORPLANS || !window.CAMPUS_FLOORPLANS['1'] || !window.CAMPUS_FLOORPLANS['2']) {
      console.error('Campus floor plan scripts did not register both building plans.');
    }
    setMode(DEFAULT_MAP_MODE);
    setBuildingMode(DEFAULT_BUILDINGS_3D);
    setOrbit(false);
  }

  function showStats(s) {
    return;
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
    openFloorPlanFromLocation();
    showStats(scene.stats);
    setStatus('');
    if (campusData && campusData.map && campusData.map.center) { HOME.center = campusData.map.center; }
    setTimeout(function () {
      var drawn = 0;
      try { drawn = map.queryRenderedFeatures({ layers: ['bld', 'bld-footprint', 'campus-bld-2d'] }).length; } catch (e) { drawn = -1; }
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