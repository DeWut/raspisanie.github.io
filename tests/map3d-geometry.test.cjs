'use strict';

const assert = require('node:assert/strict');
const sceneData = require('../data/campus-park-scene.json');
const campusData = require('../data/campus.json');
const Map3D = require('../miniapp/map3d-geometry.js');

const scene = Map3D.buildScene(sceneData, campusData, {
  renderTrees: false,
  renderWindows: false,
  renderShadows: false
});

assert.ok(scene.stats.buildings > 0, 'campus buildings should be recognised');
assert.ok(scene.footprints.length > 0, '2D building footprints should be generated');
assert.ok(scene.solid.length > 0, '3D extrusion geometry should be generated');

const mainBuilding = scene.solid.filter(function (feature) {
  return String(feature.properties && feature.properties.campus) === '1';
});
assert.ok(mainBuilding.length > 0, 'main building (campus 1) should have extrusion features');
assert.ok(mainBuilding.every(function (feature) {
  return Number(feature.properties.z1) > Number(feature.properties.z0);
}), 'main building extrusion heights must be positive');

const problems = Map3D.validateScene(scene);
assert.deepEqual(problems, [], 'generated map geometry should pass validation');

console.log(
  'Map geometry OK:',
  scene.stats.buildings + ' buildings,',
  scene.footprints.length + ' footprints,',
  scene.solid.length + ' extrusion parts.'
);
