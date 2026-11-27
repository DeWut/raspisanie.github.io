'use strict';
// Run from the repository root: node --test tests/
const test = require('node:test');
const assert = require('node:assert');
const path = require('node:path');
const G = require(path.join(__dirname, '..', 'miniapp', 'map3d-geometry.js'));

const LON0 = 30.3399, LAT0 = 59.99455;
const KX = 111320 * Math.cos(LAT0 * Math.PI / 180), KY = 111320;
const ll = (x, y) => [LON0 + x / KX, LAT0 + y / KY];
const ring = pts => { const r = pts.map(p => ll(p[0], p[1])); r.push(r[0]); return r; };
const rect = (x, y, w, h) => ring([[x, y], [x + w, y], [x + w, y + h], [x, y + h]]);
const circlePts = (cx, cy, r) => Array.from({ length: 12 }, (_, i) => [cx + Math.cos(i * Math.PI / 6) * r, cy + Math.sin(i * Math.PI / 6) * r]);

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

function allFeatures(out) {
  return [].concat(out.solid, out.windows, out.windowFrames, out.trees, out.shadows, ...Object.values(out.highlight));
}

test('height quality is classified from OSM data', () => {
  const out = G.buildScene(sampleScene(), campus);
  assert.strictEqual(out.index['1'].hq, 'known');
  assert.strictEqual(out.index['3'].hq, 'assumed');
  assert.strictEqual(out.index['3'].hm, G.DEFAULT_H);
  assert.strictEqual(out.index['4'].hm, 3.2);
  assert.strictEqual(out.stats.buildings, 4);
});

test('campus directory link is resolved by OSM id', () => {
  const out = G.buildScene(sampleScene(), campus);
  assert.strictEqual(out.index['2'].campus, '3');
});

test('ground features are passed through and trees are built', () => {
  const out = G.buildScene(sampleScene(), campus);
  assert.strictEqual(out.ground.length, 2);
  assert.strictEqual(out.stats.trees, 1);
  assert.strictEqual(out.trees.length, 4); // trunk + 3 crown tiers
});

test('every generated polygon is closed, finite and has a valid vertical range', () => {
  const out = G.buildScene(sampleScene(), campus);
  for (const f of allFeatures(out)) {
    for (const r of f.geometry.coordinates) {
      assert.ok(r.length >= 4);
      assert.deepStrictEqual(r[0], r[r.length - 1]);
      r.forEach(p => assert.ok(isFinite(p[0]) && isFinite(p[1])));
    }
    const p = f.properties;
    if (p.z1 !== undefined) { assert.ok(p.z1 > p.z0, 'z1 must be above z0'); }
    if (p.col !== undefined) { assert.match(p.col, /^#[0-9a-f]{6}$/); }
  }
});

test('courtyard buildings are not capped over the hole', () => {
  const out = G.buildScene(sampleScene(), campus);
  const parts = out.solid.filter(f => f.properties.pid === '3');
  assert.ok(parts.every(f => f.geometry.coordinates.length === 2), 'every part keeps the courtyard ring');
});

test('window budget is respected', () => {
  const out = G.buildScene(sampleScene(), campus, { maxWindows: 50 });
  assert.ok(out.windows.length <= 50);
  assert.ok(out.windowFrames.length <= 50);
  assert.ok(out.windowFrames.length > 0, 'window frames should be generated alongside glazing');
});

test('degenerate and random input does not throw or produce NaN', () => {
  let seed = 7;
  const rand = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  const features = [];
  for (let i = 0; i < 200; i++) {
    const n = 3 + Math.floor(rand() * 8);
    const pts = Array.from({ length: n }, () => [rand() * 400, rand() * 400]);
    features.push({ id: 'way/r' + i, properties: { kind: 'building', osmId: 'r' + i, heightKnown: rand() > 0.5, heightMeters: 3 + rand() * 40, heightSource: 'OSM height' }, geometry: { type: 'Polygon', coordinates: [ring(pts)] } });
  }
  features.push({ id: 'way/line', properties: { kind: 'building', osmId: 'line' }, geometry: { type: 'Polygon', coordinates: [ring([[0, 0], [10, 0], [20, 0]])] } });
  features.push({ id: 'way/tiny', properties: { kind: 'building', osmId: 'tiny' }, geometry: { type: 'Polygon', coordinates: [ring([[0, 0], [0.5, 0], [0.5, 0.5], [0, 0.5]])] } });
  features.push({ id: 'way/pt', properties: { kind: 'building', osmId: 'pt' }, geometry: { type: 'Point', coordinates: ll(0, 0) } });
  const out = G.buildScene({ features }, null);
  for (const f of allFeatures(out)) {
    JSON.stringify(f.geometry.coordinates).split(/[\[\],]+/).filter(Boolean).forEach(v => assert.ok(isFinite(Number(v))));
  }
  assert.ok(!out.index['line'] && !out.index['tiny'] && !out.index['pt']);
});