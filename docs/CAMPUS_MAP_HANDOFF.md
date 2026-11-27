# Campus map: implementation handoff

This file is the durable handoff for continuing the campus map in a GitHub-connected ChatGPT/browser session. Read it together with the current code; the code and live branch are authoritative if details diverge.

## Repository and live review

- Repository: `DeWut/raspisanie.github.io`
- Working branch: `feature/smart-campus-step-1`
- GitHub Pages site: <https://dewut.github.io/raspisanie.github.io/>
- Map UI: `miniapp/index.html`, `miniapp/app.js`, `miniapp/style.css`
- Park/building map data: `data/campus-park-scene.json`, `data/campus.json`
- Media/source notes: `data/CAMPUS_MAP_SOURCES.md`
- Building info cards: rendered in `miniapp/app.js` by `createCampusBuildingPopup()`; campus directory cards are rendered separately from `data/campus.json`.
- Test the deployed URL over HTTPS. Opening `miniapp/index.html` from disk (`file://`) blocks fetches of the GeoJSON and directory, so a blank map there does not represent the published site.

## Current implementation

- MapLibre renders the park and OSM building footprints as extruded geometry. Existing OSM paths, water, woodland, trees, and amenities are included in the locally bundled GeoJSON scene.
- Clicking an extruded building opens an information popup. It includes available OSM facts and links to OSM; the four campus buildings link to their directory entries. Campus building 1 also has an attributed Commons aerial photo, historical facts, a LAYRS source link, and an external Yandex panorama link.
- Yandex satellite mode is separate from the autonomous OSM-based scene.
- Do not recreate window rows or facade trim from floor counts. Such generic generated details were removed because they were not verified against the actual buildings.
- The Commons photograph in the building 1 information card is oblique. It is not an orthophoto, facade texture, or geometrically aligned map layer.

## Accuracy and source rules

1. Keep building footprints and tagged attributes tied to their source object. Show unknown height as unknown/illustrative; do not present estimates as measured facts.
2. Do not invent facade geometry, colors, roofs, entrances, or textures. Add facade detail only from a verifiable source: a properly licensed photogrammetric model, surveyed geometry, user-provided imagery with usage permission, or a compatible open dataset.
3. Yandex Maps/Static/Street Panorama content must only be used through the permitted Yandex API presentation and terms. Do not download, cache, stitch, extract, or bake Yandex imagery/panorama pixels into bundled textures or a derived 3D model. A link that opens the official Yandex panorama is acceptable; retain attribution and required API behavior for embedded API views.
4. OSM database content is ODbL: preserve attribution and comply with share-alike requirements for derivative databases. Read each photo/model/data source licence independently. The LAYRS site aggregates sources with different licences; use only facts whose terms permit the intended use, with attribution. Do not copy its dataset or map engine wholesale.
5. The `spb-by-building` source code is MIT-licensed, but that does not automatically license its upstream datasets or media. Review source and data licences before importing anything.
6. Never put API secrets in this file, the frontend bundle, or Git history. Keep browser keys referer-restricted and review provider usage limits.

## Known limitations and next work

- Most buildings have only simple OSM extrusions; the map does not yet have verified facade meshes or a campus-wide photogrammetric model.
- Some information is available only for the four campus buildings and OSM-tagged features. Expand building cards from properly sourced records (historical name, function, address, entrance, photo, source/license) and keep empty facts hidden.
- Inspect the live Pages version after publishing: load the map, select each of the four campus buildings and a generic OSM building, verify the popup, photo attribution, source links, and Yandex panorama link. Check both desktop and a narrow mobile viewport.
- For authentic textured 3D, first identify an openly licensed orthophoto/3D dataset covering the park or obtain permission and correctly georeferenced imagery. Then build a bounded-area pipeline that clips to the campus, records provenance, and serves optimized local assets. A single oblique photo cannot accurately texture every facade.
- Add verified entrances and routes only after checking their geometry on reliable sources. Do not infer entrances from image pixels without confirmation.
- The desktop local-file test is not a deployment test. Publish to the branch, wait for GitHub Pages Actions, then validate the HTTPS site.

## Suggested prompt for a GitHub-connected ChatGPT session

> Continue the campus map work in `DeWut/raspisanie.github.io`, branch `feature/smart-campus-step-1`. First read `docs/CAMPUS_MAP_HANDOFF.md`, `data/CAMPUS_MAP_SOURCES.md`, and inspect the latest code and Actions. Preserve existing work. Do not fabricate facade details or use Yandex imagery outside its permitted API. Work in small commits, push to this branch, wait for Pages deployment, and validate the HTTPS site. Prioritize making the existing building information popups dependable and expanding them only with source-attributed, correctly licensed data. Report what is verified versus still approximate.

## Canonical map implementation and deployment contract

The map work must be done on `feature/smart-campus-step-1`. Do not edit `main` directly. The root `index.html` is the public site entry and embeds `miniapp/map3d.html`; `miniapp/index.html` is the Mini App/native entry and embeds the same map page. A change only to `miniapp/index.html` does not update the public site's iframe. A change only to `map3d.js` does not update an already cached `map3d.html` unless the iframe URL version is also advanced.

### Files and responsibilities

| File | Responsibility | Edit when |
|---|---|---|
| `index.html` | Main website; embeds the campus map iframe | Public-site map URL, iframe sizing, or cache-busting |
| `miniapp/index.html` | Mini App and native app shell; embeds the same map | Mini App/native entry or iframe URL |
| `miniapp/map3d.html` | MapLibre container, all map CSS, controls, script includes | Map layout, mobile CSS, control contrast, script version |
| `miniapp/map3d.js` | Map lifecycle, basemap switching, 3D toggle, building markers/popups, Wikimedia, floor-plan modal | Behaviour and event handling |
| `miniapp/map3d-geometry.js` | Converts raw OSM/Overture GeoJSON into MapLibre ground, footprint and extrusion features | Geometry generation, extrusion heights, validation |
| `data/campus-park-scene.json` | Raw park/building/path/water geometry and OSM/Overture properties | Source map geometry or mapped OSM attributes |
| `data/campus.json` | Campus building directory: number, name, address, coordinates, OSM IDs, units, history, source links and photo metadata | Building labels, marker positions, room directory, historical facts |
| `miniapp/floorplans-building1.js` | Hand-redrawn SVG floor plans for building 1; registers `CAMPUS_FLOORPLANS['1']` | Building 1 plan topology, rooms, labels |
| `miniapp/floorplans.js` | Hand-redrawn SVG plans for building 2; registers `CAMPUS_FLOORPLANS['2']` | Building 2 plans |
| `service-worker.js` | Offline shell cache; caches map HTML, JS, plans and data | Every deployed map asset/cache change |
| `scripts/prepare-mobile.mjs` | Copies `miniapp/`, `data/` and service worker into Capacitor `www/` | Native packaging paths/assets |
| `.github/workflows/check-frontend.yml` | Syntax checks and map geometry regression test | Add/adjust automated checks |
| `.github/workflows/build-android.yml` | Android APK build | APK workflow or build artifact |
| `docs/floorplan-room-corrections.csv` and `.md` | Verified/manual room-name corrections | Correcting room labels without redrawing the plan |

### Data flow

1. `map3d.js` loads `data/campus-park-scene.json` and `data/campus.json` from the static site. The first is raw geometry; the second is the authoritative campus directory.
2. `Map3D.buildScene()` in `map3d-geometry.js` creates `ground`, `footprints`, `solid`, `highlight` and indexes. MapLibre receives those collections through `syncSceneData()`.
3. 3D volumes use the `solid` GeoJSON source and the `bld` `fill-extrusion` layer. A building with unknown height intentionally remains a footprint; do not claim its height or invent a measured value.
4. Markers and popup content are generated from `campus.json`. Wikimedia Commons gallery code is in `map3d.js`; building 1 is deliberately excluded from the dynamic gallery so its original local photo `miniapp/assets/campus-main-building-aerial.jpg` remains the only image in that card.
5. Floor plans are plain SVG generated by the two floor-plan JS files. The global registry is `window.CAMPUS_FLOORPLANS`, keyed by campus building number. Do not replace these vector plans with generated raster images.

### Cache/version rules — mandatory after map edits

Advance all of these together for every visible map release:
- `index.html` iframe query version for `miniapp/map3d.html`;
- `miniapp/index.html` iframe query version for `map3d.html`;
- the four script query versions at the bottom of `miniapp/map3d.html` (`map3d-geometry.js`, `floorplans.js`, `floorplans-building1.js`, `map3d.js`);
- `SHELL_CACHE` in `service-worker.js`.

The root iframe and Mini App iframe must use the same map HTML version. Do not assume that a successful commit means the live site has refreshed.

### Verification before reporting completion

Run `node --check` for `map3d.js`, `map3d-geometry.js`, both floor-plan files and `service-worker.js`; run `node tests/map3d-geometry.test.cjs`; wait for `Check frontend` and `pages build and deployment` on the exact feature-branch commit. Then test the HTTPS public site on mobile:
- open and close the map from the main page; confirm the timetable/main page is still separate;
- toggle vector scheme and satellite; confirm both are readable in dark mode;
- toggle 3D on and off in both basemaps; verify camera tilt and visible extrusions;
- tap each campus marker and open/close the building popup;
- verify building 1 shows only the original aerial photo and its attribution;
- open floor plans for buildings 1 and 2; change floors, zoom, select a room, return to map;
- check portrait mobile layout and make sure controls do not overlap.

For native app changes, wait for the Android workflow and install/test the newly built APK; browser Pages deployment does not update an already installed APK.
