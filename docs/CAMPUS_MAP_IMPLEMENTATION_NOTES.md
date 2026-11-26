# Campus map implementation notes

Working branch: `feature/campus-map-realism-scroll`
Base: `feature/smart-campus-step-1`

## Confirmed current state

- MapLibre GL JS is bundled under `miniapp/vendor`.
- Existing OSM scene has 294 features: 35 buildings, 219 paths, water, green areas and amenities.
- Of the 35 building extrusions in the checked-in scene, 5 use a directly tagged OSM height, 14 estimate height from floor count, and 16 use an invented 8 m visual default.
- The four university directory buildings are identified separately in `data/campus.json`.
- The main-building Wikimedia Commons image is oblique photography for an information card, not an orthophoto/facade texture.
- Yandex imagery/panorama must remain inside permitted official API presentation; never fetch/copy/cache its imagery as local textures.
- Copernicus DEM 30 m is far too coarse for building or roof geometry.
- A current True Colour Sentinel-2 scene was found for 2026-07-16; 10 m pixels are not enough for roofs or facade detail.
- Overture/TerraLab vector extract contains 870 building footprints in a 900 m radius, but its general footprints do not provide verified facades or reliable roof models.

## Changes in this branch so far

- Added a MapLibre raster tile source for Esri World Imagery, with visible attribution.
- Added a separate satellite/scheme toggle for the autonomous 3D park; Yandex satellite remains a separate mode.
- Improved mobile dialog layout: directory list can scroll independently from the map, and popup content has its own scroll.
- Made map canvas explicitly retain touch-pan/pinch gestures.
- Building popup now distinguishes a measured OSM height from an estimate based on floor count.
- Generator `height_for` no longer invents 8 m for a building lacking height and levels. It returns unknown, so such buildings should not be extruded.

## Remaining implementation and validation

1. Run `scripts/build-campus-park-scene.py` in a trusted environment with network access and regenerate `data/campus-park-scene.json`.
2. Validate regenerated GeoJSON: unknown heights must be null; verify the four campus IDs and their geometry remain present.
3. Update the MapLibre extrusion filter/height expression so only features with a known positive height are extruded. Do not turn unknown buildings into low/short fake blocks.
4. Build a provenance-aware Overture ingest only after verifying exact output property schema, licensing/attribution and stable feature matching. Keep Overture records distinct from original OSM relation/way IDs unless a safe match exists.
5. Investigate high-resolution photogrammetry/3D tiles for this exact campus and their usage rights. A satellite raster is a roof/ground basemap, not a facade model.
6. Check Esri World Imagery attribution and current usage/terms before public deployment; retain the source credit in the visible attribution control.
7. Validate Android Chrome and desktop: map gestures, popup scroll, directory scroll, dialog scroll, satellite toggle, Yandex official API mode, map loading and data fallback.
8. Run frontend CI and confirm GitHub Pages deployment before describing the site as updated. No deployment or browser rendering has yet been verified from this implementation session.

## Accuracy rule

Never claim the map is a photorealistic 3D model while it is only a satellite basemap plus extruded footprints. Only add facade geometry/textures from properly licensed, geometrically suitable photogrammetry, a survey, or user-authorized material. Unknown building attributes stay unknown.
