#!/usr/bin/env python3
"""Refresh the campus OSM vector snapshot used by the Cesium viewer.

Only public Overpass/OSM geometry is stored. No third-party imagery is downloaded.
Unknown building heights remain unknown; explicit building:levels are retained as
metadata but are not converted into fabricated metric heights.
"""
from __future__ import annotations

import json
import os
import sys
import time
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

BBOX = (59.9895, 30.3300, 59.9980, 30.3550)  # south, west, north, east
OUT = Path(__file__).resolve().parents[1] / "data" / "campus-open-data.geojson"
OVERPASS = os.environ.get("OVERPASS_URL", "https://overpass-api.de/api/interpreter")

def tags_to_feature(element: dict) -> dict | None:
    tags = element.get("tags", {})
    geometry = element.get("geometry") or []
    coords = [[p["lon"], p["lat"]] for p in geometry if "lon" in p and "lat" in p]
    if len(coords) < 2:
        return None
    closed = len(coords) >= 4 and coords[0] == coords[-1]
    if "building" in tags and closed:
        kind = "building"
        raw_height = tags.get("height") or tags.get("building:height")
        try:
            height = float(str(raw_height).lower().replace("meters", "").replace("meter", "").replace("m", "").strip()) if raw_height else 0
        except ValueError:
            height = 0
        height_known = 0.5 <= height <= 300
        geometry_obj = {"type": "Polygon", "coordinates": [coords]}
    elif ("landuse" in tags or tags.get("natural") in {"wood", "grassland", "water", "wetland"}) and closed:
        kind = "water" if tags.get("natural") == "water" or tags.get("water") else "green-area"
        geometry_obj = {"type": "Polygon", "coordinates": [coords]}
        height, height_known = 0, False
    elif "waterway" in tags:
        kind = "path"
        geometry_obj = {"type": "LineString", "coordinates": coords}
        height, height_known = 0, False
    elif "highway" in tags and not closed:
        kind = "path"
        geometry_obj = {"type": "LineString", "coordinates": coords}
        height, height_known = 0, False
    else:
        return None
    osm_type = element.get("type", "way")
    osm_id = element.get("id")
    props = {
        "kind": kind,
        "osmId": str(osm_id),
        "osmType": osm_type,
        "name": tags.get("name") or tags.get("name:ru") or "",
        "class": tags.get("building") or tags.get("highway") or tags.get("landuse") or tags.get("natural") or tags.get("waterway") or "",
        "heightMeters": height,
        "heightKnown": height_known,
        "heightEstimated": False,
        "source": "OpenStreetMap",
        "sourceUrl": f"https://www.openstreetmap.org/{osm_type}/{osm_id}",
    }
    if tags.get("building:levels"):
        props["buildingLevels"] = tags["building:levels"]
    return {"type": "Feature", "id": f"osm-{osm_type}-{osm_id}", "properties": props, "geometry": geometry_obj}

def main() -> int:
    south, west, north, east = BBOX
    query = f"""[out:json][timeout:90];
(
  way[building]({south},{west},{north},{east});
  way[highway]({south},{west},{north},{east});
  way[waterway]({south},{west},{north},{east});
  way[landuse]({south},{west},{north},{east});
  way[natural~"^(wood|grassland|water|wetland)$"]({south},{west},{north},{east});
);
out body geom;"""
    request = urllib.request.Request(
        OVERPASS,
        data=urllib.parse.urlencode({"data": query}).encode(),
        headers={"User-Agent": "SPbFTUIT-CampusMap/1.0 (educational campus project)", "Content-Type": "application/x-www-form-urlencoded"},
    )
    last_error = None
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                payload = json.loads(response.read().decode("utf-8"))
            break
        except Exception as exc:
            last_error = exc
            if attempt == 2:
                raise
            time.sleep(4 * (attempt + 1))
    features = [feature for element in payload.get("elements", []) if (feature := tags_to_feature(element))]
    if not features:
        raise RuntimeError("Overpass returned no usable campus features")
    result = {
        "type": "FeatureCollection",
        "name": "SPbGLTU campus OSM snapshot",
        "metadata": {
            "source": "OpenStreetMap contributors",
            "license": "ODbL-1.0",
            "retrievedAt": datetime.now(timezone.utc).isoformat(),
            "bbox": [west, south, east, north],
            "featureCount": len(features),
            "counts": {kind: sum(f["properties"]["kind"] == kind for f in features) for kind in ("building", "path", "green-area", "water")},
            "attribution": "© OpenStreetMap contributors",
        },
        "features": features,
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    temp = OUT.with_suffix(".geojson.tmp")
    temp.write_text(json.dumps(result, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")
    temp.replace(OUT)
    print(json.dumps(result["metadata"], ensure_ascii=False))
    return 0

if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"Campus OSM refresh failed: {exc}", file=sys.stderr)
        raise
