/* map3d-geometry.js
 * Builds the 3D campus scene from data/campus-park-scene.json and data/campus.json.
 * Output is plain GeoJSON for MapLibre fill-extrusion layers: building parts,
 * windows, trees, ground shadows and selection highlights.
 *
 * Only footprints and mapped heights come from OSM. Ledges, windows, cornices,
 * stepped roofs, wall colours and tree crowns are generated here and are
 * stylised, not surveyed. Buildings without a mapped height get hq = 'assumed'.
 * No DOM or network access: the file also loads in Node (see tests/).
 */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) { module.exports = factory(); }
  else { root.Map3D = factory(); }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  var FLOOR_H = 3.4;
  var DEFAULT_H = 9;
  var LOW_TYPES = { garage: 1, garages: 1, shed: 1, service: 1, kiosk: 1, roof: 1, carport: 1, hut: 1, container: 1, toilets: 1 };
  var SUN_FROM_AZ = 215;                 // azimuth the light comes from, degrees clockwise from north
  var SHADOW_AZ = (SUN_FROM_AZ + 180) % 360;

  var WALL_WARM = ['#d8d0bd', '#e3ddcf', '#c9c1b2', '#d5c9b7', '#e7e1d5', '#c6b9a7', '#d9cdbb'];
  var WALL_BRICK = ['#985f4d', '#a66a55', '#8f5949'];
  var WALL_PANEL = ['#bdbbb5', '#cbc8c0', '#b4b2ad'];

  /* ---------- small helpers ---------- */

  function makeProjector(lon0, lat0) {
    var ky = 111320;
    var kx = 111320 * Math.cos(lat0 * Math.PI / 180);
    return {
      toM: function (p) { return [(p[0] - lon0) * kx, (p[1] - lat0) * ky]; },
      toLL: function (m) {
        return [Math.round((lon0 + m[0] / kx) * 1e7) / 1e7, Math.round((lat0 + m[1] / ky) * 1e7) / 1e7];
      }
    };
  }

  function hash(str) {
    var h = 2166136261;
    str = String(str);
    for (var i = 0; i < str.length; i++) { h ^= str.charCodeAt(i); h = Math.imul(h, 16777619); }
    return h >>> 0;
  }
  function rnd(seed) { return (hash(seed) % 100000) / 100000; }
  function pick(arr, seed) { return arr[hash(seed) % arr.length]; }

  function hex2rgb(h) {
    h = String(h || '#888888').replace('#', '');
    if (h.length === 3) { h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2]; }
    var n = parseInt(h, 16);
    if (isNaN(n)) { n = 0x888888; }
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }
  function rgb2hex(c) {
    return '#' + c.map(function (v) {
      v = Math.max(0, Math.min(255, Math.round(v)));
      return (v < 16 ? '0' : '') + v.toString(16);
    }).join('');
  }
  function shade(hex, f) { return rgb2hex(hex2rgb(hex).map(function (v) { return v * f; })); }
  function lighten(hex, a) { return rgb2hex(hex2rgb(hex).map(function (v) { return v + (255 - v) * a; })); }
  function isHex(s) { return typeof s === 'string' && /^#[0-9a-f]{6}$/i.test(s.trim()); }

  function stripClose(r) {
    if (r.length > 1) {
      var a = r[0], b = r[r.length - 1];
      if (a[0] === b[0] && a[1] === b[1]) { return r.slice(0, -1); }
    }
    return r.slice();
  }
  function signedArea(r) {
    var s = 0;
    for (var i = 0; i < r.length; i++) {
      var a = r[i], b = r[(i + 1) % r.length];
      s += a[0] * b[1] - b[0] * a[1];
    }
    return s / 2;
  }
  function bbox(r) {
    var x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (var i = 0; i < r.length; i++) {
      if (r[i][0] < x0) x0 = r[i][0];
      if (r[i][0] > x1) x1 = r[i][0];
      if (r[i][1] < y0) y0 = r[i][1];
      if (r[i][1] > y1) y1 = r[i][1];
    }
    return { x0: x0, y0: y0, x1: x1, y1: y1 };
  }
  function centroid(r) {
    var a = signedArea(r);
    if (Math.abs(a) < 1e-6) {
      var sx = 0, sy = 0;
      r.forEach(function (p) { sx += p[0]; sy += p[1]; });
      return [sx / r.length, sy / r.length];
    }
    var cx = 0, cy = 0;
    for (var i = 0; i < r.length; i++) {
      var p = r[i], q = r[(i + 1) % r.length];
      var cr = p[0] * q[1] - q[0] * p[1];
      cx += (p[0] + q[0]) * cr;
      cy += (p[1] + q[1]) * cr;
    }
    return [cx / (6 * a), cy / (6 * a)];
  }
  function scaleAbout(r, sx, sy, cx, cy) {
    return r.map(function (p) { return [cx + (p[0] - cx) * sx, cy + (p[1] - cy) * sy]; });
  }
  /* Approximate offset by d metres: anisotropic scale about the bbox centre.
     Good enough for the thin trims used here (<= 0.5 m). */
  function offsetRing(r, d) {
    var b = bbox(r);
    var hx = Math.max((b.x1 - b.x0) / 2, 0.6);
    var hy = Math.max((b.y1 - b.y0) / 2, 0.6);
    return scaleAbout(r, Math.max(0.05, 1 + d / hx), Math.max(0.05, 1 + d / hy), (b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2);
  }
  function offsetRings(rings, d) {
    return rings.map(function (r, i) { return offsetRing(r, i === 0 ? d : -d); });
  }
  function hull(points) {
    var pts = points.slice().sort(function (a, b) { return a[0] - b[0] || a[1] - b[1]; });
    if (pts.length < 3) return pts;
    function cross(o, a, b) { return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]); }
    var lo = [], up = [], i;
    for (i = 0; i < pts.length; i++) {
      while (lo.length >= 2 && cross(lo[lo.length - 2], lo[lo.length - 1], pts[i]) <= 0) lo.pop();
      lo.push(pts[i]);
    }
    for (i = pts.length - 1; i >= 0; i--) {
      while (up.length >= 2 && cross(up[up.length - 2], up[up.length - 1], pts[i]) <= 0) up.pop();
      up.push(pts[i]);
    }
    up.pop(); lo.pop();
    return lo.concat(up);
  }
  function circle(cx, cy, r, n) {
    var out = [];
    for (var i = 0; i < n; i++) {
      var a = i * 2 * Math.PI / n;
      out.push([cx + Math.cos(a) * r, cy + Math.sin(a) * r]);
    }
    return out;
  }
  function closeLL(ring, P) {
    var ll = ring.map(P.toLL);
    ll.push(ll[0]);
    return ll;
  }
  function polyFeature(rings, P, props) {
    // GeoJSON / MapLibre require the exterior ring to be counter-clockwise
    // and interior rings (courtyards/holes) clockwise. OSM multipolygon
    // relations do not guarantee that input order, so normalize every
    // generated extrusion, window, tree and highlight polygon here.
    var normalized = rings.map(function (ring, index) {
      var clean = stripClose(ring);
      var area = signedArea(clean);
      var shouldBePositive = index === 0;
      if ((area > 0) !== shouldBePositive) clean.reverse();
      return closeLL(clean, P);
    });
    return { type: 'Feature', properties: props, geometry: { type: 'Polygon', coordinates: normalized } };
  }
  function allFinite(rings) {
    return rings.every(function (r) { return r.length >= 3 && r.every(function (p) { return isFinite(p[0]) && isFinite(p[1]); }); });
  }

  /* ---------- buildings ---------- */

  function wallColour(props, pid) {
    if (isHex(props.styleColor)) { return props.styleColor.trim(); }
    var clad = String(props.cladding || '').toLowerCase();
    if (/brick/.test(clad)) { return pick(WALL_BRICK, pid); }
    if (/(concrete|panel|cement)/.test(clad)) { return pick(WALL_PANEL, pid); }
    return pick(WALL_WARM, pid);
  }

  function roofColour(props, pid, shaped) {
    if (isHex(props.roofColor)) { return props.roofColor.trim(); }
    var m = String(props.roofMaterial || '').toLowerCase();
    if (/tile/.test(m)) return '#9a5a48';
    if (/metal|steel|zinc/.test(m)) return '#6f7a82';
    return shaped ? '#8a5b4d' : '#5f6368';
  }

  function heightInfo(props) {
    if (props.heightKnown && typeof props.heightMeters === 'number') {
      var fromLevels = /level|estimate/i.test(String(props.heightSource || ''));
      return { h: props.heightMeters, q: fromLevels ? 'levels' : 'known' };
    }
    var type = String(props.osmBuilding || '').toLowerCase();
    return { h: LOW_TYPES[type] ? 3.2 : DEFAULT_H, q: 'assumed' };
  }

  function windowsForRing(ring, isHole, fh, floors, seed, pid, out, frames, P, cap) {
    var ccw = signedArea(ring) > 0;
    var s = (ccw ? 1 : -1) * (isHole ? -1 : 1);
    var ww = 1.05, sp = 3.0;
    var wh = Math.min(1.55, fh * 0.43);
    for (var i = 0; i < ring.length; i++) {
      var a = ring[i], b = ring[(i + 1) % ring.length];
      var dx = b[0] - a[0], dy = b[1] - a[1];
      var L = Math.sqrt(dx * dx + dy * dy);
      if (L < 4) continue;
      var ux = dx / L, uy = dy / L;
      var nx = s * uy, ny = -s * ux;
      var count = Math.floor((L - 1.2) / sp);
      if (count < 1) continue;
      var start = (L - (count - 1) * sp) / 2;
      for (var k = 0; k < count; k++) {
        var t = start + k * sp;
        var cx = a[0] + ux * t, cy = a[1] + uy * t;
        for (var f = 0; f < floors; f++) {
          if (out.length + frames.length >= cap * 2) { return; }
          var z0 = f * fh + fh * 0.32;
          var frameW = 1.42, frameD = 0.25;
          var fx1 = cx - ux * frameW / 2 + nx * 0.025;
          var fy1 = cy - uy * frameW / 2 + ny * 0.025;
          var fx2 = cx + ux * frameW / 2 + nx * 0.025;
          var fy2 = cy + uy * frameW / 2 + ny * 0.025;
          var fx3 = [fx2 + nx * frameD, fy2 + ny * frameD];
          var fx4 = [fx1 + nx * frameD, fy1 + ny * frameD];
          var outer = [[fx1, fy1], [fx2, fy2], fx3, fx4];
          var inset = 0.13;
          var ix1 = cx - ux * (frameW / 2 - inset) + nx * 0.04;
          var iy1 = cy - uy * (frameW / 2 - inset) + ny * 0.04;
          var ix2 = cx + ux * (frameW / 2 - inset) + nx * 0.04;
          var iy2 = cy + uy * (frameW / 2 - inset) + ny * 0.04;
          var ix3 = [ix2 + nx * (frameD - 0.08), iy2 + ny * (frameD - 0.08)];
          var ix4 = [ix1 + nx * (frameD - 0.08), iy1 + ny * (frameD - 0.08)];
          frames.push(polyFeature([outer, [ix1, iy1, ix2, iy2, ix3, ix4].reverse()], P, {
            pid: pid, col: '#e4dac6', z0: z0, z1: z0 + wh + 0.16
          }));
          var p1 = [cx - ux * ww / 2 + nx * 0.055, cy - uy * ww / 2 + ny * 0.055];
          var p2 = [cx + ux * ww / 2 + nx * 0.055, cy + uy * ww / 2 + ny * 0.055];
          var p3 = [p2[0] + nx * 0.09, p2[1] + ny * 0.09];
          var p4 = [p1[0] + nx * 0.09, p1[1] + ny * 0.09];
          var r = rnd(seed + '|' + i + '|' + k + '|' + f);
          var col = r < 0.1 ? '#263849' : r < 0.45 ? '#40566c' : r < 0.8 ? '#34495d' : '#526b82';
          out.push(polyFeature([[p1, p2, p3, p4]], P, { pid: pid, col: col, z0: z0 + 0.12, z1: z0 + wh }));
        }
      }
    }
  }

  function buildScene(scene, campus, opts) {
    opts = opts || {};
    var origin = opts.origin || { lon: 30.3399, lat: 59.99455 };
    var P = makeProjector(origin.lon, origin.lat);
    var maxWin = opts.maxWindows || 30000;
    var sx = Math.sin(SHADOW_AZ * Math.PI / 180), sy = Math.cos(SHADOW_AZ * Math.PI / 180);

    var out = {
      ground: [], solid: [], windows: [], windowFrames: [], trees: [], shadows: [], highlight: {}, index: {},
      stats: { buildings: 0, known: 0, levels: 0, assumed: 0, windows: 0, trees: 0 }
    };
    var features = (scene && scene.features) || [];
    var GROUND = { 'park-boundary': 1, 'green-area': 1, water: 1, path: 1 };

    var campusByOsm = {};
    ((campus && campus.buildings) || []).forEach(function (b) {
      if (b && b.osm && b.osm.id != null) { campusByOsm[String(b.osm.id)] = b; }
    });

    function normalizeGroundFeature(feature) {
      var geometry = feature.geometry;
      function normalizePolygon(rings) {
        return rings.map(function (ring, index) {
          var clean = stripClose(ring);
          var area = signedArea(clean);
          var shouldBePositive = index === 0;
          if ((area > 0) !== shouldBePositive) clean.reverse();
          clean.push(clean[0]);
          return clean;
        });
      }
      var copy = { type: feature.type, properties: feature.properties, geometry: geometry };
      if (geometry.type === 'Polygon') {
        copy.geometry = { type: 'Polygon', coordinates: normalizePolygon(geometry.coordinates) };
      } else if (geometry.type === 'MultiPolygon') {
        copy.geometry = { type: 'MultiPolygon', coordinates: geometry.coordinates.map(normalizePolygon) };
      }
      return copy;
    }

    var buildings = [];
    features.forEach(function (f) {
      var k = f && f.properties && f.properties.kind;
      if (!k || !f.geometry) return;
      if (GROUND[k]) { out.ground.push(normalizeGroundFeature(f)); }
      else if (k === 'building') { buildings.push(f); }
    });

    function polygonsOf(geom) {
      if (geom.type === 'Polygon') return [geom.coordinates];
      if (geom.type === 'MultiPolygon') return geom.coordinates;
      return [];
    }

    // priority for the window budget: directory buildings first, then bigger footprints
    var prepared = buildings.map(function (f) {
      var polys = polygonsOf(f.geometry);
      var area = 0;
      polys.forEach(function (p) { if (p[0]) { area += Math.abs(signedArea(stripClose(p[0].map(P.toM)))); } });
      var pid = String(f.properties.osmId || f.id || hash(JSON.stringify(f.geometry)));
      var isCampus = !!(f.properties.campusNumber || campusByOsm[pid]);
      return { f: f, polys: polys, area: area, pid: pid, isCampus: isCampus };
    }).sort(function (a, b) { return (b.isCampus - a.isCampus) || (b.area - a.area); });

    prepared.forEach(function (item) {
      var props = item.f.properties;
      var pid = item.pid;
      var hi = heightInfo(props);
      var h = hi.h;
      var levels = parseInt(props.levels, 10);
      var floors = isFinite(levels) && levels > 0 ? Math.min(levels, 14) : Math.max(1, Math.round(h / FLOOR_H));
      floors = Math.min(floors, Math.max(1, Math.round(h / 2.6)));
      var fh = h / floors;
      var wall = wallColour(props, pid);
      var rawShape = String(props.roofShape || '').toLowerCase();
      var shaped = /^(hipped|gabled|pyramidal|half-hipped|gambrel|mansard|hip|gable)/.test(rawShape);
      var roof = roofColour(props, pid, shaped);
      var camp = campusByOsm[pid];
      var base = {
        pid: pid, name: props.name || '', hm: Math.round(h * 10) / 10, hq: hi.q,
        bt: props.osmBuilding || '', lv: props.levels || '', src: props.sourceUrl || '',
        campus: props.campusNumber || (camp ? String(camp.number) : '')
      };
      function part(rings, z0, z1, col) {
        if (!allFinite(rings)) return;
        var p = {}; for (var key in base) { p[key] = base[key]; }
        p.col = col; p.z0 = z0; p.z1 = z1;
        out.solid.push(polyFeature(rings, P, p));
      }

      var hl = [];
      var okPolys = 0;
      item.polys.forEach(function (poly, pi) {
        var rings = poly.map(function (r) { return stripClose(r.map(P.toM)); });
        if (!rings.length || rings[0].length < 3 || Math.abs(signedArea(rings[0])) < 4) return;
        okPolys++;
        var outer = rings[0];
        var b = bbox(outer);
        var minSide = Math.min(b.x1 - b.x0, b.y1 - b.y0);
        var hasHoles = rings.length > 1;
        var hullPts = hull(outer);
        var convex = !hasHoles && Math.abs(signedArea(outer)) / Math.max(1e-6, Math.abs(signedArea(hullPts))) >= 0.88;

        // Build facades as narrow extruded strips along every exterior and
        // courtyard edge. Extruding the entire footprint as a wall volume
        // also creates a huge horizontal top cap; on complex OSM multipolygons
        // that cap can appear as the beige slab seen in close-up screenshots.
        // Narrow wall strips preserve vertical facades without filling the
        // whole building footprint.
        var wallWidth = 0.24;
        rings.forEach(function (ring) {
          for (var wi = 0; wi < ring.length; wi++) {
            var wa = ring[wi], wb = ring[(wi + 1) % ring.length];
            var wdx = wb[0] - wa[0], wdy = wb[1] - wa[1];
            var wlen = Math.sqrt(wdx * wdx + wdy * wdy);
            if (wlen < 0.45) continue;
            var wnx = -wdy / wlen * wallWidth / 2;
            var wny = wdx / wlen * wallWidth / 2;
            var wallStrip = [
              [wa[0] + wnx, wa[1] + wny],
              [wb[0] + wnx, wb[1] + wny],
              [wb[0] - wnx, wb[1] - wny],
              [wa[0] - wnx, wa[1] - wny]
            ];
            part([wallStrip], 0, h, wall);
          }
        });

        // Roofs retain the original footprint and all courtyard holes.
        // Only explicitly tagged, convex roofs receive a restrained stepped
        // profile; unknown roofs stay flat rather than inventing geometry.
        if (shaped && convex && minSide > 4) {
          var rh = Math.max(1.2, Math.min(3.2, minSide * 0.16));
          var step = rh / 4;
          var cen = centroid(outer);
          for (var j = 0; j < 4; j++) {
            var f = 1 - 0.16 * j;
            var roofRing = scaleAbout(outer, f, f, cen[0], cen[1]);
            part([roofRing], h + j * step, h + (j + 1) * step, lighten(roof, j * 0.025));
          }
        } else {
          part(rings, h, h + 0.18, roof);
        }

        // The main historic Empire-style building gets restrained facade
        // pilasters. These are decorative vertical strips, not guessed doors.
        var isHistoric = !!(camp && camp.heritage && /ампир|empire/i.test(String(camp.heritage.style || '')));
        if (isHistoric) {
          var pilasterCount = 0;
          outer.forEach(function (a, edge) {
            if (pilasterCount >= 32) return;
            var b = outer[(edge + 1) % outer.length];
            var dx = b[0] - a[0], dy = b[1] - a[1];
            var L = Math.sqrt(dx * dx + dy * dy);
            if (L < 12) return;
            var ux = dx / L, uy = dy / L;
            var nx = (signedArea(outer) > 0 ? 1 : -1) * uy;
            var ny = (signedArea(outer) > 0 ? -1 : 1) * ux;
            var count = Math.max(1, Math.floor(L / 12));
            for (var pi = 0; pi < count && pilasterCount < 32; pi++) {
              pilasterCount++;
              var t = Math.min(L - 1.2, Math.max(1.2, (pi + 0.5) * L / (count + 1)));
              var cx = a[0] + ux * t, cy = a[1] + uy * t;
              var half = 0.16, depth = 0.11;
              var strip = [
                [cx - ux * half, cy - uy * half],
                [cx + ux * half, cy + uy * half],
                [cx + ux * half + nx * depth, cy + uy * half + ny * depth],
                [cx - ux * half + nx * depth, cy - uy * half + ny * depth]
              ];
              part([strip], 0.8, Math.max(1.2, h - 0.25), lighten(wall, 0.13));
            }
          });
        }

        // ground shadow (hull of footprint and its shifted copy when the shape is convex)
        var len = Math.min(h, 40) * 0.55;
        var shifted = outer.map(function (p) { return [p[0] + sx * len, p[1] + sy * len]; });
        var shadowRing = convex ? hull(outer.concat(shifted)) : shifted;
        if (allFinite([shadowRing])) {
          out.shadows.push(polyFeature([shadowRing], P, { k: 'b' }));
        }

        // windows
        if (h >= 4.5 && item.area >= 25 && out.windows.length < maxWin) {
          var seed = pid + ':' + pi;
          windowsForRing(outer, false, fh, floors, seed, pid, out.windows, out.windowFrames, P, maxWin);
          for (var hi2 = 1; hi2 < rings.length; hi2++) {
            windowsForRing(rings[hi2], true, fh, floors, seed + 'h' + hi2, pid, out.windows, out.windowFrames, P, maxWin);
          }
        }

        // selection highlight shape
        hl.push(polyFeature(offsetRings(rings, 0.4), P, { col: '#ffd25a', z0: 0, z1: h + 0.9 }));
      });
      if (!okPolys) return;

      out.stats.buildings++;
      out.stats[hi.q === 'known' ? 'known' : hi.q === 'levels' ? 'levels' : 'assumed']++;
      out.index[pid] = base;
      out.highlight[pid] = hl;
    });
    out.stats.windows = out.windows.length;

    // Mapped trees are kept as-is. The OSM tree layer is sparse, so fill
    // mapped woodland / arboretum polygons with deterministic illustrative trees.
    // These are vegetation-density visualizations, not surveyed individual trees.
    var TIERS = [
      { f: 1.0, z0: 2.2, z1: 5.0, l: 0 },
      { f: 0.74, z0: 5.0, z1: 7.8, l: 0.07 },
      { f: 0.46, z0: 7.8, z1: 10.4, l: 0.14 }
    ];
    var TREE_SHADOW = 5.5;
    var mappedTreePoints = [];
    features.forEach(function (f) {
      var p = f && f.properties;
      if (!p || !f.geometry || f.geometry.type !== 'Polygon') return;
      var ring = stripClose((f.geometry.coordinates[0] || []).map(P.toM));
      if (ring.length < 3) return;
      var osmId = String(p.osmId || f.id);
      if (p.kind === 'tree-trunk') {
        var tc = centroid(ring);
        if (!allFinite([ring])) return;
        mappedTreePoints.push(tc);
        out.trees.push(polyFeature([circle(tc[0], tc[1], 0.32, 8)], P, { col: '#6b5640', z0: 0, z1: 2.4 }));
      } else if (p.kind === 'tree-canopy') {
        if (!allFinite([ring])) return;
        var cen = centroid(ring);
        var v = 0.85 + rnd('tree' + osmId) * 0.4;
        var col = isHex(p.foliageColor) ? p.foliageColor : '#4f8349';
        mappedTreePoints.push(cen);
        TIERS.forEach(function (t) {
          var r = scaleAbout(ring, t.f * v, t.f * v, cen[0], cen[1]);
          out.trees.push(polyFeature([r], P, { col: lighten(col, t.l), z0: t.z0 * v, z1: t.z1 * v }));
        });
        var sh = ring.map(function (q) { return [q[0] + sx * TREE_SHADOW * v, q[1] + sy * TREE_SHADOW * v]; });
        out.shadows.push(polyFeature([hull(ring.concat(sh))], P, { k: 't' }));
        out.stats.trees++;
      }
    });

    function pointInRing(pt, ring) {
      var inside = false;
      for (var i = 0, j = ring.length - 1; i < ring.length; j = i++) {
        var a = ring[i], b = ring[j];
        var crosses = ((a[1] > pt[1]) !== (b[1] > pt[1])) &&
          (pt[0] < (b[0] - a[0]) * (pt[1] - a[1]) / ((b[1] - a[1]) || 1e-12) + a[0]);
        if (crosses) inside = !inside;
      }
      return inside;
    }
    function pointInPolygon(pt, rings) {
      if (!rings.length || !pointInRing(pt, rings[0])) return false;
      for (var h = 1; h < rings.length; h++) if (pointInRing(pt, rings[h])) return false;
      return true;
    }
    function distanceToSegment(pt, a, b) {
      var dx = b[0] - a[0], dy = b[1] - a[1];
      var den = dx * dx + dy * dy;
      var t = den ? Math.max(0, Math.min(1, ((pt[0] - a[0]) * dx + (pt[1] - a[1]) * dy) / den)) : 0;
      var x = a[0] + t * dx, y = a[1] + t * dy;
      var ex = pt[0] - x, ey = pt[1] - y;
      return Math.sqrt(ex * ex + ey * ey);
    }
    function ringDistance(pt, ring) {
      var best = Infinity;
      for (var i = 0; i < ring.length; i++) {
        best = Math.min(best, distanceToSegment(pt, ring[i], ring[(i + 1) % ring.length]));
      }
      return best;
    }
    var pathSegments = [];
    var buildingRings = [];
    features.forEach(function (f) {
      if (!f || !f.geometry) return;
      var p = f.properties || {};
      if (p.kind === 'path' && f.geometry.type === 'LineString') {
        var line = f.geometry.coordinates.map(P.toM);
        for (var i = 1; i < line.length; i++) pathSegments.push([line[i - 1], line[i]]);
      } else if (p.kind === 'building') {
        polygonsOf(f.geometry).forEach(function (poly) {
          poly.forEach(function (r) { buildingRings.push(stripClose(r.map(P.toM))); });
        });
      }
    });
    var greenFeatures = features.filter(function (f) {
      return f && f.properties && f.properties.kind === 'green-area' &&
        (/wood|forest|orchard|garden|grass|meadow/i.test(String(f.properties.areaType || 'wood')) ||
         /дендролог|дендропарк|сад/i.test(String(f.properties.name || '')));
    });
    var gardenFeatures = greenFeatures.filter(function (f) { return /дендролог|дендропарк|сад/i.test(String(f.properties.name || '')); });
    var gardenPolygons = [];
    gardenFeatures.forEach(function (f) {
      polygonsOf(f.geometry).forEach(function (poly) {
        gardenPolygons.push(poly.map(function (r) { return stripClose(r.map(P.toM)); }));
      });
    });
    var generatedPoints = [];
    var generatedLimit = 720;
    function addIllustrativeTree(x, y, seed) {
      var radius = 3.0 + rnd(seed + '|radius') * 1.15;
      var v = 0.86 + rnd(seed + '|height') * 0.28;
      var col = pick(['#356c3d', '#477e43', '#5a874b', '#3f7548', '#668c50'], seed);
      var crown = circle(x, y, radius, 10);
      out.trees.push(polyFeature([circle(x, y, 0.18, 7)], P, { col: '#66513b', z0: 0, z1: 3.0 * v }));
      TIERS.forEach(function (t) {
        var r = scaleAbout(crown, t.f, t.f, x, y);
        out.trees.push(polyFeature([r], P, { col: lighten(col, t.l), z0: t.z0 * v, z1: t.z1 * v }));
      });
      var sh = crown.map(function (q) { return [q[0] + sx * TREE_SHADOW * v, q[1] + sy * TREE_SHADOW * v]; });
      out.shadows.push(polyFeature([hull(crown.concat(sh))], P, { k: 't' }));
      generatedPoints.push([x, y]);
      out.stats.trees++;
    }
    function nearPath(pt, limit) {
      for (var i = 0; i < pathSegments.length; i++) {
        if (distanceToSegment(pt, pathSegments[i][0], pathSegments[i][1]) < limit) return true;
      }
      return false;
    }
    function nearBuilding(pt, limit) {
      for (var i = 0; i < buildingRings.length; i++) {
        if (pointInRing(pt, buildingRings[i]) || ringDistance(pt, buildingRings[i]) < limit) return true;
      }
      return false;
    }
    function nearExistingTree(pt, limit) {
      for (var i = 0; i < mappedTreePoints.length; i++) {
        var dx = pt[0] - mappedTreePoints[i][0], dy = pt[1] - mappedTreePoints[i][1];
        if (dx * dx + dy * dy < limit * limit) return true;
      }
      for (var j = 0; j < generatedPoints.length; j++) {
        var gx = pt[0] - generatedPoints[j][0], gy = pt[1] - generatedPoints[j][1];
        if (gx * gx + gy * gy < limit * limit) return true;
      }
      return false;
    }
    // Named arboretums get a dense planting pattern; the surrounding mapped
    // woodland gets a looser pattern so the park does not look empty or overplanted.
    greenFeatures.sort(function (a, b) {
      var ag = /дендролог|дендропарк|сад/i.test(String(a.properties.name || '')) ? 0 : 1;
      var bg = /дендролог|дендропарк|сад/i.test(String(b.properties.name || '')) ? 0 : 1;
      return ag - bg;
    }).forEach(function (f, fi) {
      if (generatedPoints.length >= generatedLimit) return;
      var isGarden = /дендролог|дендропарк|сад/i.test(String(f.properties.name || ''));
      var spacing = isGarden ? 10.5 : 19;
      polygonsOf(f.geometry).forEach(function (poly, pi) {
        if (!poly.length || generatedPoints.length >= generatedLimit) return;
        var rings = poly.map(function (r) { return stripClose(r.map(P.toM)); });
        var b = bbox(rings[0]);
        var jitterX = rnd(String(f.properties.osmId) + '|x|' + pi) * spacing;
        var jitterY = rnd(String(f.properties.osmId) + '|y|' + pi) * spacing;
        for (var x = b.x0 + jitterX; x <= b.x1 && generatedPoints.length < generatedLimit; x += spacing) {
          for (var y = b.y0 + jitterY; y <= b.y1 && generatedPoints.length < generatedLimit; y += spacing) {
            var seed = String(f.properties.osmId || fi) + '|' + Math.round(x) + '|' + Math.round(y);
            var pt = [x + (rnd(seed + '|jx') - 0.5) * spacing * 0.42, y + (rnd(seed + '|jy') - 0.5) * spacing * 0.42];
            if (!pointInPolygon(pt, rings)) continue;
            if (!isGarden && gardenPolygons.some(function (gr) { return pointInPolygon(pt, gr); })) continue;
            if (nearPath(pt, 3.0) || nearBuilding(pt, 3.8) || nearExistingTree(pt, isGarden ? 6.0 : 11.5)) continue;
            addIllustrativeTree(pt[0], pt[1], seed);
          }
        }
      });
    });

    return out;
  }

  return {
    buildScene: buildScene,
    makeProjector: makeProjector,
    SUN_FROM_AZ: SUN_FROM_AZ,
    FLOOR_H: FLOOR_H,
    DEFAULT_H: DEFAULT_H,
    _internals: { offsetRing: offsetRing, hull: hull, signedArea: signedArea, centroid: centroid }
  };
});