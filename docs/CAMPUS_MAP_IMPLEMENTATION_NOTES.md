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

## Follow-up fixes (2026-11-27)

- Mobile building popups are now constrained to the map viewport: 34dvh / 260 px maximum on narrow screens, with the popup content remaining vertically scrollable. The photo is capped at 112 px so the card does not cover almost the entire map.
- Desktop popup content is capped at 42dvh / 340 px. Mini App asset query versions were bumped to avoid stale cached CSS/JS.
- GitHub Actions exposed a pre-existing syntax error in `setCampusMapMode()`: a literal `\\n` sequence had been committed into the JavaScript source instead of a line break. It has been corrected; the frontend workflow must be rerun and pass before treating this branch as validated.
- The available public sources reviewed so far do not establish a downloadable, openly licensed photogrammetric mesh for the four teaching buildings. The Esri World Imagery raster is a flat aerial basemap; its source dates and resolutions vary by tile and it does not supply facade geometry. Google's Photorealistic 3D Tiles are a technically suitable online rendering candidate, but campus coverage has not been confirmed and requires an enabled Google Maps Platform project/API key. Do not claim photorealistic coverage until checked in Google's coverage viewer or via an authorised API session.
- Wikimedia Commons has additional independently attributed building photographs, but ordinary oblique photographs are not a geometrically registered texture or a photogrammetric mesh. Do not assemble a fake facade from unrelated photos. Any future photo/model import needs a source-by-source licence review and correct georeferencing.

## 3D engine decision and external imagery verification (2026-11-27)

### Recommended renderer: CesiumJS + OGC 3D Tiles
- MapLibre is suitable for 2D/2.5D vector maps and raster basemaps, but it is not the correct primary renderer for photogrammetric 3D mesh tiles.
- CesiumJS is the preferred browser renderer for this requirement. It streams hierarchical 3D Tiles, supports camera tilt/rotation/zoom, WGS84 georeferencing and can display campus OSM overlays and clickable directory markers alongside the real mesh.
- Google officially documents Photorealistic 3D Tiles as high-resolution textured 3D meshes in OGC 3D Tiles format and gives CesiumJS as a supported renderer. This is a better architecture fit than building a custom Three.js loader.
- Official references:
  - https://developers.google.com/maps/documentation/tile/3d-tiles
  - https://developers.google.com/maps/documentation/tile/3d-tiles-overview
  - https://developers.google.com/maps/documentation/tile/use-renderer
  - https://cesium.com/platform/cesiumjs/

### Coverage and licensing are hard gates
- Google's official coverage page says terrain is worldwide, but surface data (buildings and trees) is only supplied in highlighted areas and coverage is updated daily. No machine-verifiable confirmation for the exact Forestry University campus was obtained in this research session. Do not ship a Google layer as the main experience before verifying the campus in the coverage viewer or using an authorised API request.
- Google Map Tiles API requires an enabled Maps Platform project, billing and API key. For EEA developers, Google states EEA-specific terms apply from 2025-07-08 and some Map Tiles API content is no longer returned.
- Renderers must preserve Google's on-screen logo and aggregate/display tile copyright data. Do not fetch tiles for analysis, save the tiles as local assets, extract mesh geometry, bake textures, or use a screenshot/model as a derivative local model. The Google tiles are for online visualisation only.
- Official references:
  - https://developers.google.com/maps/documentation/javascript/3d/coverage
  - https://developers.google.com/maps/documentation/tile/policies
  - https://developers.google.com/maps/documentation/tile/create-renderer

### TerraLab extraction for the campus
- A new Overture/OSM extraction was requested over bbox [30.3300, 59.9895, 30.3550, 59.9980] around the Forestry University.
- TerraLab returned 291 building polygons, 841 road/path features, 112 land-use polygons and 10 water areas in approximately 1.321 km². The combined extraction reported 1,254 features total for the buildings theme query, with the above per-theme counts.
- Building-source snapshot is Overture Maps 2026-11-18, composed of OSM, Overture, Microsoft ML Building Footprints, Google Open Buildings, Esri Community Maps and other source datasets. Source/licence attribution must be kept; the building database is distributed under ODbL 1.0 share-alike conditions. See:
  - https://docs.overturemaps.org/attribution/#buildings
  - https://opendatacommons.org/licenses/odbl/1-0/
- These are footprints, not photogrammetric meshes. They improve exact footprint coverage and campus context but cannot supply true facade textures or roof forms where attributes are missing.
- TerraLab's GIS preview and direct downloads are returned in the tool response for this extraction; do not present them as already committed into this GitHub branch unless a data file is actually added and verified.

### Public imagery reference
- Aerial reference image of the historic main building was found at:
  https://smapse.com/storage/2022/11/converted/660_464_2-7.jpg
- It clearly shows the U-shaped main building and the internal courtyard, but is an oblique aerial photograph hosted by a third party. Its source and licence/permission for reuse in this app were not verified. Do not download, store, or use it as a texture unless permission and redistribution rights are confirmed.
- The existing Commons image remains the only currently documented licensed photograph in the repository, and it is an information-card image, not registered geometry.

### Architecture plan for next implementation phase
1. Add a dedicated CesiumJS map renderer as a separate mode, retaining current directory/search/popups as business logic and preserving the current MapLibre/OSM viewer as an offline/fallback mode.
2. Build a small provider module that reads an owner-supplied, restricted browser key from runtime configuration (not committed to Git). It should only be used for the online photorealistic 3D Tiles rendering route.
3. Do not make a Google layer mandatory. If coverage fails or no key exists, keep the existing offline campus scene and show a clear diagnostic message.
4. Load the OSM/Overture building footprints and campus directory as vector overlays on Cesium, not as replacements for the photogrammetric mesh. Use OSM IDs to match the four campus building cards where a match is known.
5. Keep the existing map controls, directory scroll and popup scroll independent. Add a mobile fullscreen / expand action for the map to make camera gestures usable on small screens.
6. Keep an explicit attribution component visible and do not suppress Cesium/Google credits.
7. Add a visual source status ("Photorealistic 3D tiles", "OSM footprint overlay", "offline schematic fallback") so users know what they are looking at.
8. Before deployment, validate 3D coverage at [30.3300,59.9895]-[30.3550,59.9980], mobile GPU performance, camera gesture conflicts, loading errors, key restrictions and Actions. Do not call the map photorealistic when only the fallback is active.

### Current stopping point
The data source/engine research is complete enough to choose CesiumJS, but actual photorealistic campus coverage and an authorised Google API key are not verified/available in this session. The next code step should be the Cesium viewer shell and provider-config fallback; the final photorealistic layer remains gated by the user's Google Maps Platform project/API key and coverage confirmation.

## CesiumJS viewer shell implemented (2026-11-27)

- Added a dedicated "Фотореалистичный 3D" mode and CesiumJS renderer host to the campus map dialog. The existing MapLibre offline/OSM scene and Yandex satellite mode remain separate.
- Added lazy CesiumJS loading from the official CesiumJS distribution and a Google Photorealistic 3D Tiles root tileset request. The integration explicitly enables on-screen credit display and uses the tile service only as a live visualisation.
- Added `miniapp/campus-map-config.js` as a no-key runtime config stub and a configuration guard. No Google key, credentials, or secrets were added to Git. When no key is present, the UI explains the requirement and the local map remains available.
- Added a separate Cesium viewport, responsive styles, attribution area, Service Worker shell caching for the configuration stub, and CI syntax checking for the new config file.
- The Cesium mode is an integration shell only until a valid browser-restricted key is supplied and the precise campus coverage is confirmed. Google tile loading has not yet been tested against a real project/key.
- The Cesium scene is not yet carrying the university's OSM footprints, markers, search or building information popups as vector overlays. That remains the next code task after proving the photorealistic tiles load.
- No local image or mesh has been derived from third-party map imagery. Google imagery/geometry stays remote and is only rendered in the authorised online viewer.
