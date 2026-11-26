"""Download a small, reproducible OpenStreetMap scene for the university park."""
from __future__ import annotations

import json
import math
import re
import urllib.request
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "data" / "campus-park-scene.json"
PARK_RELATION = "5622070"
# This padded extract contains the complete named park relation and its ways.
BBOX = (30.3290, 59.9885, 30.3555, 59.9985)
MAP_URL = "https://api.openstreetmap.org/api/0.6/map?bbox=" + ",".join(map(str, BBOX))


def read_tags(element: ET.Element) -> dict[str, str]:
    return {tag.get("k", ""): tag.get("v", "") for tag in element.findall("tag")}


def point_in_ring(point: tuple[float, float], ring: list[list[float]]) -> bool:
    x, y = point
    inside = False
    previous = ring[-1]
    for current in ring:
        x1, y1 = previous
        x2, y2 = current
        if ((y1 > y) != (y2 > y)) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1:
            inside = not inside
        previous = current
    return inside


def stitch_role_rings(relation: ET.Element, role: str, ways: dict[str, ET.Element], nodes: dict[str, list[float]]) -> list[list[list[float]]]:
    segments: list[list[str]] = []
    for member in relation.findall("member"):
        if member.get("type") != "way" or member.get("role") != role:
            continue
        way = ways.get(member.get("ref", ""))
        if way is None:
            continue
        refs = [node.get("ref", "") for node in way.findall("nd")]
        if len(refs) >= 2 and all(ref in nodes for ref in refs):
            segments.append(refs)

    rings: list[list[list[float]]] = []
    while segments:
        ring = segments.pop(0)
        while ring[-1] != ring[0]:
            match_index = next((i for i, part in enumerate(segments) if part[0] == ring[-1] or part[-1] == ring[-1]), None)
            if match_index is None:
                raise ValueError(f"Could not close {role} ring for relation {relation.get('id', '')}")
            part = segments.pop(match_index)
            if part[-1] == ring[-1]:
                part.reverse()
            ring.extend(part[1:])
        coordinates = [[round(float(nodes[ref][0]), 7), round(float(nodes[ref][1]), 7)] for ref in ring]
        if len(coordinates) >= 4:
            rings.append(coordinates)
    return rings


def height_for(tags: dict[str, str]) -> tuple[float, str]:
    match = re.search(r"\d+(?:[.,]\d+)?", tags.get("height", ""))
    if match:
        return max(3.5, min(70.0, float(match.group().replace(",", ".")))), "OSM height"
    levels = re.search(r"\d+", tags.get("building:levels", tags.get("levels", "")))
    if levels:
        return max(3.5, min(70.0, int(levels.group()) * 3.4)), "OSM levels × 3.4 m"
    return 8.0, "visual default; height not mapped in OSM"


def make_multipolygon(outer_rings: list[list[list[float]]], inner_rings: list[list[list[float]]]) -> list[list[list[list[float]]]]:
    polygons = [[outer] for outer in outer_rings]
    for inner in inner_rings:
        sample = (sum(point[0] for point in inner[:-1]) / max(1, len(inner) - 1),
                  sum(point[1] for point in inner[:-1]) / max(1, len(inner) - 1))
        for polygon in polygons:
            if point_in_ring(sample, polygon[0]):
                polygon.append(inner)
                break
    return polygons


def tree_crown(lon: float, lat: float, radius_m: float) -> list[list[float]]:
    longitude_radius = radius_m / (111_320 * max(0.2, math.cos(math.radians(lat))))
    latitude_radius = radius_m / 111_320
    ring = [[round(lon + math.cos(i * math.tau / 12) * longitude_radius, 7),
             round(lat + math.sin(i * math.tau / 12) * latitude_radius, 7)] for i in range(12)]
    return ring + [ring[0]]


def main() -> None:
    request = urllib.request.Request(
        MAP_URL,
        headers={"User-Agent": "SPbFTUIT-campus-scene/1.0 (https://github.com/DeWut/raspisanie.github.io)"},
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        root = ET.fromstring(response.read())

    nodes = {
        element.get("id", ""): [round(float(element.get("lon", "0")), 7), round(float(element.get("lat", "0")), 7)]
        for element in root.findall("node")
    }
    ways = {element.get("id", ""): element for element in root.findall("way")}
    relations = {element.get("id", ""): element for element in root.findall("relation")}
    relation = relations.get(PARK_RELATION)
    if relation is None:
        raise ValueError(f"OSM extract did not include park relation {PARK_RELATION}")

    park_rings = stitch_role_rings(relation, "outer", ways, nodes)
    if not park_rings:
        raise ValueError("No complete park boundary ring was returned by OpenStreetMap")
    park_ring = max(park_rings, key=len)
    park_tags = read_tags(relation)
    park_name = park_tags.get("name:ru") or park_tags.get("name") or "Парк Лесотехнической академии"
    features: list[dict] = [{
        "type": "Feature",
        "id": f"relation/{PARK_RELATION}",
        "properties": {"kind": "park-boundary", "name": park_name, "osmId": PARK_RELATION},
        "geometry": {"type": "Polygon", "coordinates": [park_ring]},
    }]
    counts: dict[str, int] = {"park-boundary": 1}

    # Preserve mapped forest clearings as holes instead of painting the whole woodland solid.
    for forest_relation in relations.values():
        forest_tags = read_tags(forest_relation)
        if forest_tags.get("natural") != "wood" and forest_tags.get("landuse") != "forest":
            continue
        try:
            outer_rings = stitch_role_rings(forest_relation, "outer", ways, nodes)
            inner_rings = stitch_role_rings(forest_relation, "inner", ways, nodes)
        except ValueError:
            continue  # Skip incomplete relations returned by a clipped extract.
        if not outer_rings:
            continue
        polygons = make_multipolygon(outer_rings, inner_rings)
        sample = outer_rings[0][len(outer_rings[0]) // 2]
        if not point_in_ring((sample[0], sample[1]), park_ring):
            continue
        forest_id = forest_relation.get("id", "")
        features.append({
            "type": "Feature", "id": f"relation/{forest_id}",
            "properties": {"kind": "green-area", "areaType": "wood", "name": forest_tags.get("name:ru") or forest_tags.get("name", ""), "osmId": forest_id,
                           "sourceUrl": f"https://www.openstreetmap.org/relation/{forest_id}"},
            "geometry": {"type": "MultiPolygon", "coordinates": polygons},
        })
        counts["green-area"] = counts.get("green-area", 0) + 1

    for building_relation in relations.values():
        building_tags = read_tags(building_relation)
        if "building" not in building_tags:
            continue
        try:
            outer_rings = stitch_role_rings(building_relation, "outer", ways, nodes)
            inner_rings = stitch_role_rings(building_relation, "inner", ways, nodes)
        except ValueError:
            continue
        if not outer_rings:
            continue
        sample_ring = outer_rings[0]
        sample = (sum(point[0] for point in sample_ring[:-1]) / max(1, len(sample_ring) - 1),
                  sum(point[1] for point in sample_ring[:-1]) / max(1, len(sample_ring) - 1))
        if not point_in_ring(sample, park_ring):
            continue
        building_id = building_relation.get("id", "")
        height, height_source = height_for(building_tags)
        explicit_colour = building_tags.get("building:colour", building_tags.get("colour", ""))
        palette = ["#dfcba8", "#c98768", "#e5d9bf", "#b98568", "#d5bd96"]
        palette_index = sum(ord(char) for char in building_id) % len(palette)
        features.append({
            "type": "Feature", "id": f"relation/{building_id}",
            "properties": {"kind": "building", "osmId": building_id,
                           "name": building_tags.get("name:ru") or building_tags.get("name", ""),
                           "osmBuilding": building_tags.get("building", ""), "levels": building_tags.get("building:levels", ""),
                           "heightMeters": height, "heightSource": height_source,
                           "styleColor": explicit_colour if re.fullmatch(r"#[0-9a-fA-F]{6}", explicit_colour) else palette[palette_index],
                           "cladding": building_tags.get("building:cladding", building_tags.get("cladding", "")),
                           "roofShape": building_tags.get("roof:shape", ""),
                           "sourceUrl": f"https://www.openstreetmap.org/relation/{building_id}"},
            "geometry": {"type": "MultiPolygon", "coordinates": make_multipolygon(outer_rings, inner_rings)},
        })
        counts["building"] = counts.get("building", 0) + 1

    amenity_icons = {"bench": "bench", "drinking_water": "water", "bicycle_parking": "bike", "waste_basket": "bin"}
    for element in root:
        if element.tag not in ("node", "way"):
            continue
        osm_id = element.get("id", "")
        tags = read_tags(element)
        if element.tag == "node":
            coordinate = nodes.get(osm_id)
            if coordinate is None or not point_in_ring((coordinate[0], coordinate[1]), park_ring):
                continue
            if tags.get("natural") == "tree":
                kind = "tree"
            elif tags.get("amenity") in amenity_icons:
                kind = "amenity"
            else:
                continue
            properties = {"kind": kind, "osmId": osm_id, "amenity": amenity_icons.get(tags.get("amenity"), ""),
                          "name": tags.get("name:ru") or tags.get("name", "")}
            if kind == "tree":
                crown_diameter = re.search(r"\d+(?:[.,]\d+)?", tags.get("diameter_crown", ""))
                crown_radius = max(2.2, min(5.0, float(crown_diameter.group().replace(",", ".")) / 2)) if crown_diameter else 3.2
                foliage_palette = ["#3f7744", "#54884b", "#628f50", "#417354"]
                foliage_color = foliage_palette[sum(ord(char) for char in osm_id) % len(foliage_palette)]
                properties.update({"kind": "tree-canopy", "canopyBaseMeters": 2.2, "heightMeters": 6.5,
                                   "foliageColor": foliage_color,
                                   "treeShapeNote": "illustrative crown size; OSM maps only the tree point"})
                geometry = {"type": "Polygon", "coordinates": [tree_crown(coordinate[0], coordinate[1], crown_radius)]}
                features.append({"type": "Feature", "id": f"node/{osm_id}-trunk", "properties": {
                    "kind": "tree-trunk", "osmId": osm_id, "heightMeters": 2.2,
                    "sourceUrl": f"https://www.openstreetmap.org/node/{osm_id}"},
                    "geometry": {"type": "Polygon", "coordinates": [tree_crown(coordinate[0], coordinate[1], 0.35)]}})
            else:
                geometry = {"type": "Point", "coordinates": coordinate}
        else:
            refs = [nd.get("ref", "") for nd in element.findall("nd")]
            if len(refs) < 2 or any(ref not in nodes for ref in refs):
                continue
            coordinates = [nodes[ref] for ref in refs]
            center = (sum(point[0] for point in coordinates) / len(coordinates), sum(point[1] for point in coordinates) / len(coordinates))
            if not point_in_ring(center, park_ring):
                continue
            closed = coordinates[0] == coordinates[-1]
            kind = ""
            if tags.get("highway") in {"footway", "path", "pedestrian", "steps", "cycleway"}:
                kind = "path"
            elif closed and (tags.get("natural") in {"wood", "scrub", "water"} or tags.get("landuse") in {"forest", "grass", "meadow"} or tags.get("leisure") == "garden"):
                kind = "green-area" if tags.get("natural") in {"wood", "scrub"} or tags.get("landuse") in {"forest", "grass", "meadow"} or tags.get("leisure") == "garden" else "water"
            elif closed and "building" in tags:
                kind = "building"
            else:
                continue
            properties = {"kind": kind, "osmId": osm_id, "name": tags.get("name:ru") or tags.get("name", ""),
                          "osmBuilding": tags.get("building", ""), "levels": tags.get("building:levels", tags.get("levels", "")),
                          "surface": tags.get("surface", ""), "colour": tags.get("building:colour", tags.get("colour", "")),
                          "heightMeters": height_for(tags)[0], "heightSource": height_for(tags)[1]}
            explicit_colour = tags.get("building:colour", tags.get("colour", ""))
            palette = ["#dfcba8", "#c98768", "#e5d9bf", "#b98568", "#d5bd96"]
            palette_index = sum(ord(char) for char in osm_id) % len(palette)
            properties["styleColor"] = explicit_colour if re.fullmatch(r"#[0-9a-fA-F]{6}", explicit_colour) else palette[palette_index]
            properties["cladding"] = tags.get("building:material", tags.get("cladding", ""))
            properties["roofShape"] = tags.get("roof:shape", "")
            geometry = {"type": "Polygon", "coordinates": [coordinates]} if closed and kind != "path" else {"type": "LineString", "coordinates": coordinates}

        source_type = "node" if element.tag == "node" else "way"
        properties["sourceUrl"] = f"https://www.openstreetmap.org/{source_type}/{osm_id}"
        features.append({"type": "Feature", "id": f"{source_type}/{osm_id}", "properties": properties, "geometry": geometry})
        counts[kind] = counts.get(kind, 0) + 1

    bounds = [
        [round(min(point[0] for point in park_ring), 7), round(min(point[1] for point in park_ring), 7)],
        [round(max(point[0] for point in park_ring), 7), round(max(point[1] for point in park_ring), 7)],
    ]
    document = {
        "type": "FeatureCollection",
        "metadata": {"name": park_name, "source": "OpenStreetMap API", "osmRelation": PARK_RELATION,
                     "retrieved": datetime.now(timezone.utc).strftime("%Y-%m-%d"), "license": "ODbL-1.0",
                     "attribution": "© OpenStreetMap contributors", "copyrightUrl": "https://www.openstreetmap.org/copyright",
                     "bounds": bounds, "counts": counts,
                     "note": "Mapped geometry is from OSM. Buildings without a mapped height use a clearly marked visual default; trees appear only at mapped OSM tree points."},
        "features": features,
    }
    OUT.write_text(json.dumps(document, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"Wrote {OUT.relative_to(ROOT)}: {len(features)} features; {counts}; bounds={bounds}")


if __name__ == "__main__":
    main()
