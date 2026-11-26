# Campus map implementation notes

Working branch: `feature/campus-map-realism-scroll`
Base branch: `feature/smart-campus-step-1`

## Verified starting point

- Repository: `DeWut/raspisanie.github.io`.
- MapLibre GL JS is bundled under `miniapp/vendor`.
- Existing scene: 294 features — 35 building footprints, 219 paths, water, green areas, trees and amenities.
- The four university directory buildings are identified separately in `data/campus.json`.
- The main-building Wikimedia Commons image is an oblique aerial photograph for an information card, not an orthophoto or a georeferenced facade texture.
- Yandex satellite/panorama remains an official API/link mode. Do not fetch, copy, cache, stitch or bake its imagery into local texture assets.
- Copernicus DEM GLO-30 has about 30 m resolution, too coarse for building/roof geometry.
- A Sentinel-2 true-colour scene from 2026-07-16 was found, but its 10 m pixels are not enough for roofs or facades.
- TerraLab/Overture extraction returned 870 building footprints in a 900 m radius. Those footprints are not verified facade meshes or photogrammetric models.
- Esri World Imagery is a tiled aerial/satellite image basemap, not a 3D model. The exact capture date and native resolution over this campus still need checking from the service's citation metadata.

## Changes committed in this branch

- Added an Esri World Imagery raster tile source and an independent scheme/satellite toggle for the MapLibre campus map. The map's attribution control receives a service credit string. Yandex mode remains separate.
- Mobile map dialog is now a bounded flex layout. The campus directory gets an independent vertical scroll surface; MapLibre has its own gesture surface; popup content scrolls inside its own height limit.
- Campus directory popups and map-feature popups distinguish direct OSM height from a floor-count estimate.
- Unknown-height building features remain selectable and show a footprint, not a fabricated 8 m extrusion.
- Extruded massing is limited to features with `heightKnown: true`. Buildings with unknown height remain footprint-only.
- Existing random/material-inferred facade colours were removed from the checked-in scene. The generator now supplies a facade colour only when an OSM colour tag is explicitly present; otherwise the map uses a neutral schematic massing colour.
- Current OSM feature snapshot has 35 buildings: 5 with directly tagged height, 14 with floor-count estimate and 16 without height. `data/campus-park-scene.json` preserves the source retrieval date (2026-11-26) separately from the height audit date (2026-11-27).
- Added a visible status breakdown so users can tell how many buildings have tagged heights, estimated heights, or footprints only.
- Added this implementation/limitations record.

## Realistic 3D imagery research

### Candidate: Google Photorealistic 3D Tiles

Google's Map Tiles API offers photorealistic 3D meshes textured by aerial imagery, served in OGC 3D Tiles. This is the closest technical match to the requested satellite-like real-world 3D scene.

However:
- Photorealistic surface coverage is not worldwide; Google says terrain is global but 3D surface coverage is only in highlighted regions and changes over time.
- Coverage over the Forestry University campus has not been confirmed. The official coverage page uses an interactive embedded map that was not readable through the current research interface.
- Use requires a Google Maps Platform project, billing, an enabled Map Tiles API, and an API key/session.
- Google requires its logo and aggregated per-tile copyright attributions. The client must render the service; imagery/model tiles must not be extracted, cached outside the permitted rules, measured, reprocessed, converted into a derived GLB or used to build a local facade model.
- Therefore this could only be a separate online visualisation mode, not a way to generate reusable campus geometry.

References:
- https://developers.google.com/maps/documentation/tile/quick-reference
- https://developers.google.com/maps/documentation/javascript/3d/coverage
- https://developers.google.com/maps/documentation/tile/3d-tiles-overview
- https://developers.google.com/maps/documentation/tile/policies

### Other city-scale data

- Anyscape lists a Saint Petersburg city model based on survey/open GIS data and Overture/OSM with LoD1 building massing. It is a commercial/request-a-quote product and does not establish photoreal facade coverage for the university.
- The Saint Petersburg 3D urban-planning model described by IGIT is based on a 1:10,000 topographic plan with remotely sensed building heights. This may be useful for city-scale massing, but no openly downloadable campus photomesh was established.
- Public/photogrammetric imagery and user photo references are not interchangeable: a single oblique photograph cannot accurately texture every side of a building.

## Remaining work before calling the map complete

1. Verify Esri tile usage/attribution against current Esri terms and inspect its imagery source, resolution and capture date for this exact campus. Do not assume the global basemap is uniformly high resolution.
2. Check Google's live Photorealistic 3D Tiles coverage specifically at the campus coordinates. If the area is covered, decide whether an optional Google-powered visualisation mode is acceptable, obtain an owner-approved API key and configure billing/usage restrictions. Do not put secrets in GitHub.
3. If Google coverage is absent or the terms do not fit, obtain a licensed photogrammetric/CityGML/3D Tiles source for the campus or user-authorized photographs/scan data. No verified, openly licensed facade mesh for this exact university has yet been found.
4. Run `scripts/build-campus-park-scene.py` in a network-enabled environment after the next OSM refresh. Verify that unknown heights stay null, the four campus building IDs remain present, and the explicit-colour-only rule is preserved.
5. Consider adding mapped `building:part`, `roof:shape`, `roof:height` and roof-part geometry only after validating those tags exist for these specific buildings. Do not infer them from a photo or the number of floors.
6. Test the page on Android Chrome/Telegram and desktop: map pan, rotate, pinch zoom, map popup scroll, directory scroll, modal sizing, satellite toggle, offline fallback and Yandex mode.
7. Check GitHub Actions. The current repository workflow only targets specific branches; no successful action run or GitHub Pages deployment has been observed for this work branch. Do not describe the hosted site as updated until the actual Pages URL is checked.

## Accuracy rule

Do not describe a satellite raster plus extruded building footprints as photorealistic 3D. The current work improves source transparency, satellite context, mobile usability and honest massing. Authentic facade/roof photogrammetry is a separate data-acquisition/integration step and remains unfinished.