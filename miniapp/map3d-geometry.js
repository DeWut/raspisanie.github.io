/* map3d-geometry.js
 * Builds the 3D campus scene from data/campus-park-scene.json and data/campus.json.
 * Output is plain GeoJSON for MapLibre fill-extrusion layers: building parts,
 * windows, trees, ground shadows and selection highlights.
 *
 * Only footprints and mapped heights come from OSM. Ledges, windows, cornices,
 * stepped roofs, wall colours and tree crowns are generated here and are
 * stylised, not surveyed. Buildings without a mapped height get hq = 'assumed'.
 * No DOM or network access: the file also loads in Node (see tests/).
 *
 * Every polygon leaves this module through polyFeature(), which rejects rings
 * with non-finite coordinates, and through sanitize(), which drops anything
 * larger than a campus-sized shape. A single malformed ring in a fill-extrusion
 * source is enough to draw screen-sized slabs, so both gates are deliberate.
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
  var MAX_SPAN_DEG = { lon: 0.012, lat: 0.007 };   // ~700 m: larger than any single building or tree

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
        return [Math.round((lon0 + m[0] / kx) * 1e8) / 1e8, Math.round((lat0 + m[1] / ky) * 1e8) / 1e8];
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
  /* Drops consecutive duplicate vertices (< 1 cm apart) so edge normals stay defined. */
  function dedupe(r) {
    var out = [];
    for (var i = 0; i < r.length; i++) {
      var p = r[i], q = out[out.length - 1];
      if (!q || Math.abs(p[0] - q[0]) > 0.01 || Math.abs(p[1] - q[1]) > 0.01) { out.push(p); }
    }
    while (out.length > 1 && Math.abs(out[0][0] - out[out.length - 1][0]) <= 0.01 && Math.abs(out[0][1] - out[out.length - 1][1]) <= 0.01) { out.pop(); }
    return out;
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

  /* +1 / -1: which side of an edge points away from the solid.
     Outer ring: outward. Hole ring: into the hole. The sign accounts for ring winding. */
  function outwardSign(ring, isHole) {
    return (signedArea(ring) > 0 ? 1 : -1) * (isHole ? -1 : 1);
  }

  /* Mitred offset of one ring by d metres (d > 0 grows the solid, d < 0 shrinks it).
     Unlike scaling about the bbox centre, every edge moves by the same distance,
     so trims never end up coplanar with the wall on complex footprints. */
  function offsetRing(ring, d, isHole) {
    var n = ring.length, s = outwardSign(ring, isHole), nor = [], out = [], i;
    for (i = 0; i < n; i++) {
      var a = ring[i], b = ring[(i + 1) % n];
      var dx = b[0] - a[0], dy = b[1] - a[1], L = Math.sqrt(dx * dx + dy * dy) || 1;
      nor.push([s * dy / L, -s * dx / L]);
    }
    for (i = 0; i < n; i++) {
      var n0 = nor[(i + n - 1) % n], n1 = nor[i];
      var den = 1 + n0[0] * n1[0] + n0[1] * n1[1];
      var mx, my;
      if (den < 0.25) { mx = n1[0]; my = n1[1]; }
      else { mx = (n0[0] + n1[0]) / den; my = (n0[1] + n1[1]) / den; }
      var len = Math.sqrt(mx * mx + my * my);
      if (len > 2.5) { mx *= 2.5 / len; my *= 2.5 / len; }
      out.push([ring[i][0] + mx * d, ring[i][1] + my * d]);
    }
    return out;
  }
  /* Returns null when an inset would collapse a thin wing. */
  function offsetRings(rings, d) {
    if (d < 0) {
      var b = bbox(rings[0]);
      if (Math.min(b.x1 - b.x0, b.y1 - b.y0) < -2 * d + 1) { return null; }
    }
    return rings.map(function (r, i) { return offsetRing(r, d, i > 0); });
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
    var ll = ring.map(function (m) { return P.toLL(m); });
    ll.push(ll[0]);
    return ll;
  }

  /* The only way a polygon is built. Returns null for anything malformed:
     rings that are not arrays of [x, y] numbers, non-finite values, or
     zero-area shapes. Exterior ring is wound counter-clockwise, holes clockwise. */
  function polyFeature(rings, P, props) {
    var coords = [];
    for (var i = 0; i < rings.length; i++) {
      var clean = stripClose(rings[i]);
      if (clean.length < 3) { if (i === 0) return null; continue; }
      for (var k = 0; k < clean.length; k++) {
        var p = clean[k];
        if (!p || typeof p[0] !== 'number' || typeof p[1] !== 'number' || !isFinite(p[0]) || !isFinite(p[1])) { return null; }
      }
      var area = signedArea(clean);
      if (Math.abs(area) < 1e-6) { if (i === 0) return null; continue; }
      if ((area > 0) !== (i === 0)) { clean.reverse(); }
      coords.push(closeLL(clean, P));
    }
    if (!coords.length) { return null; }
    return { type: 'Feature', properties: props, geometry: { type: 'Polygon', coordinates: coords } };
  }

  function featureIsSane(f) {
    var g = f && f.geometry;
    if (!g || g.type !== 'Polygon' || !g.coordinates.length) { return false; }
    var x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
    for (var i = 0; i < g.coordinates.length; i++) {
      var r = g.coordinates[i];
      if (r.length < 4) { return false; }
      for (var k = 0; k < r.length; k++) {
        var x = r[k][0], y = r[k][1];
        if (typeof x !== 'number' || typeof y !== 'number' || !isFinite(x) || !isFinite(y)) { return false; }
        if (x < x0) x0 = x; if (x > x1) x1 = x;
        if (y < y0) y0 = y; if (y > y1) y1 = y;
      }
    }
    return (x1 - x0) <= MAX_SPAN_DEG.lon && (y1 - y0) <= MAX_SPAN_DEG.lat;
  }
  /* Last line of defence: removes any generated polygon that is invalid or absurdly large. */
  function sanitize(list, stats) {
    var kept = [];
    for (var i = 0; i < list.length; i++) {
      if (featureIsSane(list[i])) { kept.push(list[i]); } else { stats.dropped++; }
    }
    return kept;
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

  /* One window = a cream surround (a shallow solid box, a little larger than the glass)
     plus a dark glass box standing slightly in front of it. Both are plain
     four-point rectangles: no holes, so nothing can go wrong in triangulation.
     deco.pilaster(rect, zTop) receives wall strips for historic facades. */
  function windowsForRing(ring, isHole, fh, floors, seed, pid, glass, frames, add, cap, deco) {
    var s = outwardSign(ring, isHole);
    var gw = 1.05, fw = 1.3, sp = 3.0;
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
      var box = function (cx, cy, w, d0, d1) {
        return [
          [cx - ux * w / 2 + nx * d0, cy - uy * w / 2 + ny * d0],
          [cx + ux * w / 2 + nx * d0, cy + uy * w / 2 + ny * d0],
          [cx + ux * w / 2 + nx * d1, cy + uy * w / 2 + ny * d1],
          [cx - ux * w / 2 + nx * d1, cy - uy * w / 2 + ny * d1]
        ];
      };
      for (var k = 0; k < count; k++) {
        var cx = a[0] + ux * (start + k * sp), cy = a[1] + uy * (start + k * sp);
        for (var f = 0; f < floors; f++) {
          if (glass.length >= cap) { return; }
          var z0 = f * fh + fh * 0.32;
          add(frames, [box(cx, cy, fw, 0.01, 0.07)], { pid: pid, col: '#e4dac6', z0: z0 - 0.08, z1: z0 + wh + 0.1 });
          var r = rnd(seed + '|' + i + '|' + k + '|' + f);
          var col = r < 0.1 ? '#263849' : r < 0.45 ? '#40566c' : r < 0.8 ? '#34495d' : '#526b82';
          add(glass, [box(cx, cy, gw, 0.07, 0.11)], { pid: pid, col: col, z0: z0, z1: z0 + wh });
        }
        if (deco && deco.pilaster && k % 4 === 3 && k + 1 < count) {
          var tx = a[0] + ux * (start + (k + 0.5) * sp), ty = a[1] + uy * (start + (k + 0.5) * sp);
          deco.pilaster([box(tx, ty, 0.34, 0.01, 0.12)]);
        }
      }
    }
  }

  function buildScene(scene, campus, opts) {
    opts = opts || {};
    var origin = opts.origin || { lon: 30.3399, lat: 59.99455 };
    var P = makeProjector(origin.lon, origin.lat);
    var maxWin = opts.maxWindows || 14000;
    var sx = Math.sin(SHADOW_AZ * Math.PI / 180), sy = Math.cos(SHADOW_AZ * Math.PI / 180);

    var out = {
      ground: [], solid: [], windows: [], windowFrames: [], trees: [], shadows: [], highlight: {}, index: {},
      stats: { buildings: 0, known: 0, levels: 0, assumed: 0, windows: 0, trees: 0, dropped: 0 }
    };
    var features = (scene && scene.features) || [];
    var GROUND = { 'park-boundary': 1, 'green-area': 1, water: 1, path: 1 };

    function add(list, rings, props) {
      var f = polyFeature(rings, P, props);
      if (f) { list.push(f); } else { out.stats.dropped++; }
      return f;
    }

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
          if ((area > 0) !== (index === 0)) clean.reverse();
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
      var plinth = shade(wall, 0.74);
      var ledge = lighten(wall, 0.08);
      var trim = lighten(wall, 0.2);
      var rawShape = String(props.roofShape || '').toLowerCase();
      var shaped = /^(hipped|gabled|pyramidal|half-hipped|gambrel|mansard|hip|gable)/.test(rawShape);
      var roof = roofColour(props, pid, shaped);
      var camp = campusByOsm[pid];
      var isHistoric = !!(camp && camp.heritage && /ампир|empire/i.test(String(camp.heritage.style || '')));
      var base = {
        pid: pid, name: props.name || '', hm: Math.round(h * 10) / 10, hq: hi.q,
        bt: props.osmBuilding || '', lv: props.levels || '', src: props.sourceUrl || '',
        campus: props.campusNumber || (camp ? String(camp.number) : '')
      };
      function part(rings, z0, z1, col) {
        if (!rings) return;
        var p = {}; for (var key in base) { p[key] = base[key]; }
        p.col = col; p.z0 = z0; p.z1 = z1;
        add(out.solid, rings, p);
      }

      var hl = [];
      var okPolys = 0;
      item.polys.forEach(function (poly, pi) {
        var rings = poly.map(function (r) { return dedupe(stripClose(r.map(P.toM))); })
          .filter(function (r) { return r.length >= 3; });
        if (!rings.length || Math.abs(signedArea(rings[0])) < 4) return;
        okPolys++;
        var outer = rings[0];
        var b = bbox(outer);
        var minSide = Math.min(b.x1 - b.x0, b.y1 - b.y0);
        var hasHoles = rings.length > 1;
        var hullPts = hull(outer);
        var convex = !hasHoles && Math.abs(signedArea(outer)) / Math.max(1e-6, Math.abs(signedArea(hullPts))) >= 0.88;

        // Massing. The walls are one extrusion of the real footprint including
        // courtyard holes; trims are mitred offsets so they never share a plane with the wall.
        if (h >= 4) { part(offsetRings(rings, 0.12), 0, 1.0, plinth); }
        part(rings, 0, h, wall);
        for (var i = 1; i < floors; i++) { part(offsetRings(rings, 0.07), i * fh - 0.12, i * fh + 0.1, ledge); }
        part(offsetRings(rings, 0.25), h - 0.3, h + 0.35, trim);

        // Roof. Only explicitly tagged convex roofs get a stepped profile;
        // everything else stays a flat roof with a parapet lip.
        if (shaped && convex && minSide > 4) {
          var rh = Math.max(1.5, Math.min(4.5, minSide * 0.22));
          var step = rh / 4;
          var cen = centroid(outer);
          for (var j = 0; j < 4; j++) {
            var f = 1 - 0.2 * j;
            part([scaleAbout(outer, f, f, cen[0], cen[1])], h + 0.35 + j * step, h + 0.35 + (j + 1) * step, lighten(roof, j * 0.03));
          }
        } else {
          part(offsetRings(rings, -0.4) || rings, h + 0.35, h + 0.5, roof);
        }

        // ground shadow (hull of footprint and its shifted copy when the shape is convex)
        var len = Math.min(h, 40) * 0.55;
        var shifted = outer.map(function (p) { return [p[0] + sx * len, p[1] + sy * len]; });
        add(out.shadows, [convex ? hull(outer.concat(shifted)) : shifted], { k: 'b' });

        // windows (and pilasters between them on the historic building)
        if (h >= 4.5 && item.area >= 25 && out.windows.length < maxWin) {
          var seed = pid + ':' + pi;
          var deco = isHistoric ? { pilaster: function (r) { part(r, 0.9, Math.max(1.4, h - 0.3), lighten(wall, 0.14)); } } : null;
          windowsForRing(outer, false, fh, floors, seed, pid, out.windows, out.windowFrames, add, maxWin, deco);
          for (var hi2 = 1; hi2 < rings.length; hi2++) {
            windowsForRing(rings[hi2], true, fh, floors, seed + 'h' + hi2, pid, out.windows, out.windowFrames, add, maxWin, deco);
          }
        }

        // selection highlight shape
        var hlRings = offsetRings(rings, 0.4);
        if (hlRings) { add(hl, hlRings, { col: '#ffd25a', z0: 0, z1: h + 0.9 }); }
      });
      if (!okPolys) return;

      out.stats.buildings++;
      out.stats[hi.q === 'known' ? 'known' : hi.q === 'levels' ? 'levels' : 'assumed']++;
      out.index[pid] = base;
      out.highlight[pid] = hl;
    });
    out.stats.windows = out.windows.length;

    // Mapped trees are kept as-is. The OSM tree layer is sparse, so mapped woodland
    // and arboretum polygons are filled with deterministic illustrative trees.
    // These visualise vegetation density; they are not surveyed individual trees.
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
        mappedTreePoints.push(tc);
        add(out.trees, [circle(tc[0], tc[1], 0.32, 8)], { col: '#6b5640', z0: 0, z1: 2.4 });
      } else if (p.kind === 'tree-canopy') {
        var cen = centroid(ring);
        var v = 0.85 + rnd('tree' + osmId) * 0.4;
        var col = isHex(p.foliageColor) ? p.foliageColor : '#4f8349';
        mappedTreePoints.push(cen);
        TIERS.forEach(function (t) {
          add(out.trees, [scaleAbout(ring, t.f * v, t.f * v, cen[0], cen[1])], { col: lighten(col, t.l), z0: t.z0 * v, z1: t.z1 * v });
        });
        var sh = ring.map(function (q) { return [q[0] + sx * TREE_SHADOW * v, q[1] + sy * TREE_SHADOW * v]; });
        add(out.shadows, [hull(ring.concat(sh))], { k: 't' });
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
    var GARDEN_RE = /дендролог|дендропарк|сад/i;
    var greenFeatures = features.filter(function (f) {
      return f && f.properties && f.properties.kind === 'green-area' &&
        (/wood|forest|orchard|garden|grass|meadow/i.test(String(f.properties.areaType || 'wood')) ||
         GARDEN_RE.test(String(f.properties.name || '')));
    });
    var gardenPolygons = [];
    greenFeatures.filter(function (f) { return GARDEN_RE.test(String(f.properties.name || '')); }).forEach(function (f) {
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
      add(out.trees, [circle(x, y, 0.18, 7)], { col: '#66513b', z0: 0, z1: 3.0 * v });
      TIERS.forEach(function (t) {
        add(out.trees, [scaleAbout(crown, t.f, t.f, x, y)], { col: lighten(col, t.l), z0: t.z0 * v, z1: t.z1 * v });
      });
      var sh = crown.map(function (q) { return [q[0] + sx * TREE_SHADOW * v, q[1] + sy * TREE_SHADOW * v]; });
      add(out.shadows, [hull(crown.concat(sh))], { k: 't' });
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
      var ag = GARDEN_RE.test(String(a.properties.name || '')) ? 0 : 1;
      var bg = GARDEN_RE.test(String(b.properties.name || '')) ? 0 : 1;
      return ag - bg;
    }).forEach(function (f, fi) {
      if (generatedPoints.length >= generatedLimit) return;
      var isGarden = GARDEN_RE.test(String(f.properties.name || ''));
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

    // Final gate: nothing invalid or oversized reaches MapLibre.
    out.solid = sanitize(out.solid, out.stats);
    out.windows = sanitize(out.windows, out.stats);
    out.windowFrames = sanitize(out.windowFrames, out.stats);
    out.trees = sanitize(out.trees, out.stats);
    out.shadows = sanitize(out.shadows, out.stats);
    Object.keys(out.highlight).forEach(function (k) { out.highlight[k] = sanitize(out.highlight[k], out.stats); });
    out.stats.windows = out.windows.length;

    return out;
  }

  /* Returns a list of human-readable problems found in a built scene (empty = fine). */
  function validateScene(out) {
    var problems = [];
    var lists = { solid: out.solid, windows: out.windows, windowFrames: out.windowFrames, trees: out.trees, shadows: out.shadows };
    Object.keys(lists).forEach(function (name) {
      lists[name].forEach(function (f, i) {
        if (!featureIsSane(f)) { problems.push(name + '[' + i + '] is invalid or oversized'); return; }
        var p = f.properties || {};
        if (p.z1 !== undefined && !(p.z1 > p.z0)) { problems.push(name + '[' + i + '] has z1 <= z0'); }
      });
    });
    return problems;
  }

  return {
    buildScene: buildScene,
    validateScene: validateScene,
    makeProjector: makeProjector,
    SUN_FROM_AZ: SUN_FROM_AZ,
    FLOOR_H: FLOOR_H,
    DEFAULT_H: DEFAULT_H,
    _internals: { offsetRing: offsetRing, offsetRings: offsetRings, hull: hull, signedArea: signedArea, centroid: centroid, polyFeature: polyFeature }
  };
});