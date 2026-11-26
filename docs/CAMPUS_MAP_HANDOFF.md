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
