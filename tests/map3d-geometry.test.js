'use strict';
// Run from the repository root: node --test tests/map3d-geometry.test.js
const test = require('node:test');
const assert = require('node:assert');
const path = require('node:path');
const G = require(path.join(__dirname, '..', 'miniapp', 'map3d-geometry.js'));

const LON0 = 30.3399, LAT0 = 59.99455;
const KX = 111320 * Math.cos(LAT0 * Math.PI / 180), KY = 111320;
const ll = (x, y) => [LON0 + x / KX, LAT0 + y / KY];
const toM = p => [(p[0] - LON0) * KX, (p[1] - LAT0) * KY];
const ring = pts => { const r = pts.map(p => ll(p[0], p[1])); r.push(r[0]); return r; };
const rect = (x, y, w, h) => ring([[x, y], [x + w, y], [x + w, y + h], [x, y + h]]);
const circlePts = (cx, cy, r, n = 12) => Array.from({ length: n }, (_, i) => [cx + Math.cos(i * 2 * Math.PI / n) * r, cy + Math.sin(i * 2 * Math.PI / n) * r]);
const areaOf = r => r.slice(0, -1).reduce((s, p, i, pts) => { const q = pts[(i + 1) % pts.length]; return s + p[0] * q[1] - q[0] * p[1]; }, 0) / 2;

function sampleScene() {
  return {
    features: [
      { id: 'way/1', properties: { kind: 'building', osmId: '1', heightKnown: true, heightMeters: 18.5, heightSource: 'OSM height', levels: '4' }, geometry: { type: 'Polygon', coordinates: [rect(0, 0, 40, 15)] } },
      { id: 'way/2', properties: { kind: 'building', osmId: '2', heightKnown: true, heightMeters: 10, heightSource: 'OSM height', roofShape: 'hipped', campusNumber: '3' }, geometry: { type: 'Polygon', coordinates: [rect(60, 0, 20, 12)] } },
      { id: 'relation/3', properties: { kind: 'building', osmId: '3', heightKnown: false, heightMeters: null, osmBuilding: 'university' },
        geometry: { type: 'MultiPolygon', coordinates: [[rect(0, 40, 60, 60), ring(circlePts(30, 70, 8)).reverse()]] } },
      { id: 'way/4', properties: { kind: 'building', osmId: '4', heightKnown: false, osmBuilding: 'garage' }, geometry: { type: 'Polygon', coordinates: [rect(100, 0, 6, 3)] } },
      { id: 'node/9-trunk', properties: { kind: 'tree-trunk', osmId: '9' }, geometry: { type: 'Polygon', coordinates: [ring(circlePts(120, 50, 0.35))] } },
      { id: 'node/9', properties: { kind: 'tree-canopy', osmId: '9', foliageColor: '#3f7744' }, geometry: { type: 'Polygon', coordinates: [ring(circlePts(120, 50, 3.2))] } },
      { id: 'way/5', properties: { kind: 'path' }, geometry: { type: 'LineString', coordinates: [ll(0, 0), ll(10, 10)] } },
      { id: 'way/6', properties: { kind: 'water' }, geometry: { type: 'Polygon', coordinates: [rect(200, 0, 10, 10)] } }
    ]
  };
}
const campus = { buildings: [{ number: '3', osm: { id: 2 } }] };

// Courtyard building shaped like the main corpus: outer ring with a courtyard,
// stored in the "wrong" winding on purpose (OSM does not guarantee it).
const COURT_OUTER = [[0, 0], [0, 90], [140, 90], [140, 0]];            // clockwise
const COURT_HOLE = [[25, 20], [115, 20], [115, 70], [25, 70]];         // counter-clockwise
function courtyardScene(extraProps) {
  return {
    features: [{
      id: 'relation/2045600',
      properties: Object.assign({ kind: 'building', osmId: '2045600', heightKnown: true, heightMeters: 16, levels: '3', campusNumber: '1' }, extraProps || {}),
      geometry: { type: 'MultiPolygon', coordinates: [[ring(COURT_OUTER), ring(COURT_HOLE)]] }
    }]
  };
}
const empireCampus = { buildings: [{ number: '1', osm: { id: 2045600 }, heritage: { style: 'Ампир' } }] };

function allFeatures(out) {
  return [].concat(out.footprints || [], out.solid, out.windows, out.windowFrames, out.trees, out.shadows, ...Object.values(out.highlight));
}
function centroidLL(f) {
  const r = f.geometry.coordinates[0].slice(0, -1);
  return toM([r.reduce((s, p) => s + p[0], 0) / r.length, r.reduce((s, p) => s + p[1], 0) / r.length]);
}
function inRing(pt, r) {
  let inside = false;
  for (let i = 0, j = r.length - 1; i < r.length; j = i++) {
    const a = r[i], b = r[j];
    if ((a[1] > pt[1]) !== (b[1] > pt[1]) && pt[0] < (b[0] - a[0]) * (pt[1] - a[1]) / (b[1] - a[1]) + a[0]) inside = !inside;
  }
  return inside;
}

test('height quality is classified from OSM data', () => {
  const out = G.buildScene(sampleScene(), campus);
  assert.strictEqual(out.index['1'].hq, 'known');
  assert.strictEqual(out.index['3'].hq, 'unknown');
  assert.strictEqual(out.index['3'].hm, 0);
  assert.strictEqual(out.index['4'].hq, 'unknown');
  assert.strictEqual(out.index['4'].hm, 0);
  assert.ok(out.ground.some(f => f.properties.kind === 'building-footprint' && f.properties.pid === '3'));
  assert.ok(!out.solid.some(f => f.properties.pid === '3'), 'unknown height must not create a fake extrusion');
  assert.strictEqual(out.stats.buildings, 4);
});

test('sub-square-metre mapped footprints are not silently omitted from the building count', () => {
  const input = sampleScene();
  input.features.push({
    id: 'way/tiny',
    properties: { kind: 'building', osmId: 'tiny', heightKnown: false },
    geometry: { type: 'Polygon', coordinates: [rect(130, 0, 0.9, 0.7)] }
  });
  const out = G.buildScene(input, campus);
  assert.strictEqual(out.stats.buildings, 5);
  assert.ok(out.ground.some(f => f.properties.kind === 'building-footprint' && f.properties.pid === 'tiny'));
});

test('flat building footprints preserve known heights and courtyard holes', () => {
  const out = G.buildScene(courtyardScene(), empireCampus, {
    renderTrees: false, renderWindows: false, renderShadows: false
  });
  assert.strictEqual(out.footprints.length, 1);
  assert.strictEqual(out.footprints[0].properties.pid, '2045600');
  assert.strictEqual(out.footprints[0].geometry.coordinates.length, 2, 'the courtyard remains a hole in 2D mode');
  assert.strictEqual(out.footprints[0].properties.heightQuality, 'known');
  assert.deepStrictEqual(G.validateScene(out), []);
});

test('campus directory link is resolved by OSM id', () => {
  const out = G.buildScene(sampleScene(), campus);
  assert.strictEqual(out.index['2'].campus, '3');
});

test('REGRESSION: every polygon, including window frames, is finite, closed and has z1 > z0', () => {
  const out = G.buildScene(sampleScene(), campus);
  assert.ok(out.windowFrames.length > 0, 'frames must be generated');
  assert.deepStrictEqual(G.validateScene(out), []);
  assert.strictEqual(out.stats.dropped, 0, 'nothing should be dropped on valid input');
  for (const f of allFeatures(out)) {
    for (const r of f.geometry.coordinates) {
      assert.ok(r.length >= 4);
      assert.deepStrictEqual(r[0], r[r.length - 1]);
      r.forEach(p => assert.ok(isFinite(p[0]) && isFinite(p[1]), 'NaN / null coordinate'));
    }
    const p = f.properties;
    if (p.z1 !== undefined) assert.ok(p.z1 > p.z0);
    if (p.col !== undefined) assert.match(p.col, /^#[0-9a-f]{6}$/);
  }
});

test('REGRESSION: a ring that mixes numbers and points (the old frame bug) is rejected, not emitted', () => {
  const P = G.makeProjector(LON0, LAT0);
  const outer = [[0, 0], [1, 0], [1, 1], [0, 1]];
  const broken = [0.2, 0.2, 0.8, 0.2, [0.8, 0.8], [0.2, 0.8]];   // what b7085a0 built for the frame hole
  assert.strictEqual(G._internals.polyFeature([outer, broken], P, {}), null);
  assert.strictEqual(G._internals.polyFeature([outer, [[0.2, 0.2], [NaN, 0.2], [0.8, 0.8]]], P, {}), null);
  assert.ok(G._internals.polyFeature([outer], P, {}));
});

test('windows and frames are simple rectangles, one frame per glass pane', () => {
  const out = G.buildScene(sampleScene(), campus);
  assert.ok(out.windowFrames.length > out.windows.length, 'separate frame members surround each glass pane');
  for (const f of out.windows.concat(out.windowFrames)) {
    assert.strictEqual(f.geometry.coordinates.length, 1, 'no holes in window geometry');
    assert.strictEqual(f.geometry.coordinates[0].length, 5);
  }
});

test('windows sit on the facade outside the wall volume, never inside it', () => {
  const out = G.buildScene(courtyardScene(), null);
  const outer = COURT_OUTER, hole = COURT_HOLE;
  assert.ok(out.windows.length > 100);
  for (const f of out.windows) {
    const c = centroidLL(f);
    const insideSolid = inRing(c, outer) && !inRing(c, hole);
    assert.ok(!insideSolid, 'window centre must be outside the solid footprint');
  }
  assert.ok(out.windows.some(f => inRing(centroidLL(f), hole)), 'courtyard facades also get windows');
});

test('courtyard building (reversed winding): walls and roof keep the courtyard hole', () => {
  const out = G.buildScene(courtyardScene(), null);
  const body = out.solid.find(f => f.properties.z0 === 0 && f.properties.z1 === 16);
  assert.ok(body, 'solid wall volume exists');
  assert.strictEqual(body.geometry.coordinates.length, 2, 'courtyard is a real hole');
  assert.ok(areaOf(body.geometry.coordinates[0]) > 0, 'outer ring counter-clockwise');
  assert.ok(areaOf(body.geometry.coordinates[1]) < 0, 'hole clockwise');
  const roof = out.solid.find(f => Math.abs(f.properties.z0 - 16.35) < 1e-9 && Math.abs(f.properties.z1 - 16.5) < 1e-9);
  assert.ok(roof && roof.geometry.coordinates.length === 2, 'flat roof plate keeps the hole');
  const cornice = out.solid.find(f => Math.abs(f.properties.z0 - 15.7) < 1e-9);
  assert.ok(cornice && cornice.geometry.coordinates.length === 2, 'cornice keeps the hole');
});

test('no part of a building extends beyond its footprint by more than a trim width', () => {
  const out = G.buildScene(courtyardScene(), empireCampus);
  const maxOut = 0.6;      // plinth 0.12, ledge 0.07, cornice 0.25, highlight 0.4, windows 0.12
  const xs = COURT_OUTER.map(p => p[0]), ys = COURT_OUTER.map(p => p[1]);
  const x0 = Math.min(...xs) - maxOut, x1 = Math.max(...xs) + maxOut, y0 = Math.min(...ys) - maxOut, y1 = Math.max(...ys) + maxOut;
  const parts = out.solid.concat(out.windows, out.windowFrames, out.highlight['2045600']);
  assert.ok(parts.length > 0);
  for (const f of parts) {
    for (const p of f.geometry.coordinates[0]) {
      const m = toM(p);
      assert.ok(m[0] >= x0 && m[0] <= x1 && m[1] >= y0 && m[1] <= y1, 'part escapes the footprint bbox: ' + JSON.stringify(m));
    }
  }
});

test('floor ledges and cornice are rings around the facade, not slabs across the courtyard', () => {
  const out = G.buildScene(courtyardScene(), null);
  const bands = out.solid.filter(f => f.properties.z1 - f.properties.z0 < 0.8 && f.properties.z0 > 1.2);
  assert.ok(bands.length >= 3);
  assert.ok(bands.every(f => f.geometry.coordinates.length === 2), 'every horizontal band keeps the courtyard hole');
});

test('mitred offset moves every edge by the same distance', () => {
  const r = [[0, 0], [40, 0], [40, 20], [0, 20]];
  const grown = G._internals.offsetRing(r, 0.5, false);
  assert.ok(Math.abs(Math.abs(G._internals.signedArea(grown)) - 41 * 21) < 1e-6);
  const lshape = [[0, 0], [40, 0], [40, 10], [10, 10], [10, 30], [0, 30]];
  const off = G._internals.offsetRing(lshape, 0.3, false);
  const dist = (p, a, b) => { const dx = b[0] - a[0], dy = b[1] - a[1], t = Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy))); return Math.hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy); };
  for (const p of off) {
    const d = Math.min(...lshape.map((a, i) => dist(p, a, lshape[(i + 1) % lshape.length])));
    assert.ok(d > 0.25 && d < 0.5, 'vertex should be ~0.3-0.42 m from the original outline, got ' + d);
  }
});

test('insets that would collapse a thin wing are skipped instead of producing junk', () => {
  assert.strictEqual(G._internals.offsetRings([[[0, 0], [30, 0], [30, 1.5], [0, 1.5]]], -0.5), null);
  const thin = { features: [{ id: 'way/thin', properties: { kind: 'building', osmId: 'thin', heightKnown: true, heightMeters: 6 }, geometry: { type: 'Polygon', coordinates: [rect(0, 0, 30, 1.5)] } }] };
  const out = G.buildScene(thin, null);
  assert.deepStrictEqual(G.validateScene(out), []);
  assert.ok(out.solid.length > 0);
});

test('tagged convex hipped roof is stepped; unknown roofs stay flat', () => {
  const out = G.buildScene(sampleScene(), campus);
  const hipped = out.solid.filter(f => f.properties.pid === '2' && f.properties.z0 >= 10.35 - 1e-9);
  assert.strictEqual(hipped.length, 4);
  const flat = out.solid.filter(f => f.properties.pid === '1' && f.properties.z0 >= 18.85 - 1e-9);
  assert.strictEqual(flat.length, 1);
});

test('historic building gets pilasters between windows, other buildings do not', () => {
  const historic = G.buildScene(courtyardScene(), empireCampus);
  const plain = G.buildScene(courtyardScene(), null);
  const pilasters = o => o.solid.filter(f => f.properties.z0 === 0.9);
  assert.ok(pilasters(historic).length > 4);
  assert.strictEqual(pilasters(plain).length, 0);
});

test('oversized or corrupted geometry is dropped by the sanitize gate', () => {
  const huge = { features: [{ id: 'way/huge', properties: { kind: 'building', osmId: 'huge', heightKnown: true, heightMeters: 10 }, geometry: { type: 'Polygon', coordinates: [rect(0, 0, 5000, 5000)] } }] };
  const out = G.buildScene(huge, null);
  assert.strictEqual(out.solid.length, 0, 'a 5 km "building" must not reach the map');
  assert.ok(out.stats.dropped > 0);
});

test('ground features are passed through and trees are built', () => {
  const input = sampleScene();
  const expectedGround = input.features.filter(f => ['park-boundary', 'green-area', 'water', 'path'].includes(f.properties.kind)).length;
  const expectedFootprints = input.features.filter(f => {
    const p = f.properties || {};
    const hasHeight = p.heightKnown && typeof p.heightMeters === 'number' && p.heightMeters > 0;
    const hasLevels = Number.isFinite(parseInt(p.levels, 10)) && parseInt(p.levels, 10) > 0;
    return p.kind === 'building' && !hasHeight && !hasLevels;
  }).length;
  const out = G.buildScene(input, campus);
  assert.strictEqual(out.ground.filter(f => f.properties.kind !== 'building-footprint').length, expectedGround);
  assert.strictEqual(out.ground.filter(f => f.properties.kind === 'building-footprint').length, expectedFootprints);
  assert.strictEqual(out.stats.trees, 1);
  assert.ok(out.trees.length >= 4, 'a mapped tree point receives trunk and crown layers');
});

test('named arboretum polygons generate clustered trees while respecting paths', () => {
  const garden = {
    features: [{
      id: 'relation/garden',
      properties: { kind: 'green-area', areaType: 'wood', name: 'Верхний дендрологический сад', osmId: 'garden' },
      geometry: { type: 'Polygon', coordinates: [rect(-50, -50, 100, 100)] }
    }, {
      id: 'way/path',
      properties: { kind: 'path' },
      geometry: { type: 'LineString', coordinates: [ll(-50, 0), ll(50, 0)] }
    }]
  };
  const out = G.buildScene(garden, null);
  assert.ok(out.stats.estimatedTrees > 0, 'named arboretums receive clearly-labelled estimated placements');
  assert.strictEqual(out.stats.mappedTrees, 0, 'these test placements are not surveyed OSM tree points');
  assert.strictEqual(out.stats.trees, out.stats.estimatedTrees);
  assert.ok(out.trees.length >= out.stats.estimatedTrees * 5, 'each estimated tree has a trunk and layered crown');
  const pathY = 0;
  for (const f of out.trees) {
    if (f.properties.z0 === 0) {   // trunks mark the planting point
      const c = centroidLL(f);
      assert.ok(Math.abs(c[1] - pathY) >= 3.5, 'estimated trees must not stand on the path');
    }
  }
  assert.deepStrictEqual(G.validateScene(out), []);
});

test('ground polygons are normalized even when OSM rings have mixed winding', () => {
  const scene = { features: [{
    id: 'relation/ground',
    properties: { kind: 'green-area', areaType: 'wood' },
    geometry: { type: 'Polygon', coordinates: [ring([[0, 0], [0, 20], [20, 20], [20, 0]]), ring([[5, 5], [15, 5], [15, 15], [5, 15]]).reverse()] }
  }] };
  const out = G.buildScene(scene, null);
  const polygon = out.ground[0].geometry.coordinates;
  assert.ok(areaOf(polygon[0]) > 0);
  assert.ok(areaOf(polygon[1]) < 0);
});

test('window frames are built from separate narrow members rather than a solid backing slab', () => {
  const out = G.buildScene(sampleScene(), campus);
  assert.ok(out.windowFrames.length > out.windows.length, 'four frame members and a sill accompany each glazed window');
});

test('window budget is respected', () => {
  const out = G.buildScene(sampleScene(), campus, { maxWindows: 50 });
  assert.ok(out.windows.length <= 50);
  assert.ok(out.windowFrames.length <= 150);
  assert.ok(out.windowFrames.length > 0);
});

test('degenerate and random input does not throw, and the output always validates', () => {
  let seed = 7;
  const rand = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  const features = [];
  for (let i = 0; i < 200; i++) {
    const n = 3 + Math.floor(rand() * 8);
    const pts = Array.from({ length: n }, () => [rand() * 400, rand() * 400]);
    features.push({ id: 'way/r' + i, properties: { kind: 'building', osmId: 'r' + i, heightKnown: rand() > 0.5, heightMeters: 3 + rand() * 40, heightSource: 'OSM height' }, geometry: { type: 'Polygon', coordinates: [ring(pts)] } });
  }
  features.push({ id: 'way/line', properties: { kind: 'building', osmId: 'line' }, geometry: { type: 'Polygon', coordinates: [ring([[0, 0], [10, 0], [20, 0]])] } });
  features.push({ id: 'way/tiny', properties: { kind: 'building', osmId: 'tiny' }, geometry: { type: 'Polygon', coordinates: [ring([[0, 0], [0.3, 0], [0.3, 0.3], [0, 0.3]])] } });
  features.push({ id: 'way/pt', properties: { kind: 'building', osmId: 'pt' }, geometry: { type: 'Point', coordinates: ll(0, 0) } });
  features.push({ id: 'way/dup', properties: { kind: 'building', osmId: 'dup', heightKnown: true, heightMeters: 12 }, geometry: { type: 'Polygon', coordinates: [ring([[0, 0], [0, 0], [30, 0], [30, 0], [30, 20], [0, 20]])] } });
  const out = G.buildScene({ features }, null);
  assert.deepStrictEqual(G.validateScene(out), []);
  assert.ok(!out.index['line'] && !out.index['tiny'] && !out.index['pt']);
  assert.ok(out.index['dup'], 'duplicate vertices must not break a building');
});