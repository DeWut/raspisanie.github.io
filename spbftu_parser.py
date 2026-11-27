#!/usr/bin/env python3
"""
edu.spbftu.ru Moodle scraper (legitimate session only)
Usage:
  python spbftu_parser.py --user your@email.ru --password 'pass'
"""

import argparse
import json
import re
import sys
from pathlib import Path
from urllib.parse import urljoin, urlparse, parse_qs

import requests
from bs4 import BeautifulSoup

BASE = "https://edu.spbftu.ru"
SESSION = requests.Session()
SESSION.headers.update({
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
})
# Moodle иногда ругается на verify, но лучше оставить True
SESSION.verify = True


def get_logintoken() -> str:
    r = SESSION.get(f"{BASE}/login/index.php")
    r.raise_for_status()
    soup = BeautifulSoup(r.text, "html.parser")
    token = soup.find("input", {"name": "logintoken"})
    if not token or not token.get("value"):
        # иногда cookie-banner, повторный GET после «согласия» не нужен для requests
        raise RuntimeError("logintoken not found")
    return token["value"]


def login(username: str, password: str) -> None:
    token = get_logintoken()
    data = {
        "anchor": "",
        "logintoken": token,
        "username": username,
        "password": password,
    }
    r = SESSION.post(
        f"{BASE}/login/index.php",
        data=data,
        allow_redirects=True,
    )
    r.raise_for_status()

    # успех: есть MoodleSession и мы не на странице логина
    if "MoodleSession" not in SESSION.cookies.get_dict():
        raise RuntimeError("Login failed: no MoodleSession cookie")
    if "/login/" in r.url:
        raise RuntimeError("Login failed: still on login page (wrong credentials?)")
    print(f"[+] Logged in as {username}")


def get_my_courses() -> list[dict]:
    """Список курсов с /my/ и /course/index.php"""
    courses = {}
    for path in ("/my/", "/course/index.php"):
        r = SESSION.get(f"{BASE}{path}")
        r.raise_for_status()
        soup = BeautifulSoup(r.text, "html.parser")
        for a in soup.select("a[href*='/course/view.php']"):
            href = a.get("href", "")
            m = re.search(r"[?&]id=(\d+)", href)
            if not m:
                continue
            cid = m.group(1)
            title = a.get_text(strip=True)
            if not title or title.lower() in ("course", "курс"):
                continue
            courses[cid] = {
                "id": cid,
                "title": title,
                "url": urljoin(BASE, href),
            }
    return list(courses.values())


def parse_course(course_id: str) -> dict:
    """Содержимое курса: секции + активности"""
    url = f"{BASE}/course/view.php?id={course_id}"
    r = SESSION.get(url)
    r.raise_for_status()
    soup = BeautifulSoup(r.text, "html.parser")

    title = soup.find("h1")
    course = {
        "id": course_id,
        "title": title.get_text(strip=True) if title else f"Course {course_id}",
        "url": url,
        "sections": [],
    }

    # стандартные секции Moodle
    for section in soup.select("li.section, div.section, .course-content li.section"):
        sec_id = section.get("id", "")
        sec_name = section.select_one(".sectionname, .section-title, h3, h4")
        section_data = {
            "id": sec_id,
            "name": sec_name.get_text(strip=True) if sec_name else "",
            "activities": [],
        }

        for act in section.select("li.activity, .activity, .modtype_assign, .modtype_quiz, .modtype_resource, .modtype_folder, .modtype_url, .modtype_page, .modtype_label"):
            link = act.select_one("a[href*='/mod/']")
            if not link:
                # label / текст без ссылки
                text = act.get_text(" ", strip=True)
                if text:
                    section_data["activities"].append({
                        "type": "label",
                        "title": text[:200],
                        "url": None,
                    })
                continue

            href = link.get("href", "")
            name = link.get_text(strip=True)
            mod = re.search(r"/mod/(\w+)/", href)
            mod_type = mod.group(1) if mod else "unknown"
            cmid = None
            m = re.search(r"[?&]id=(\d+)", href)
            if m:
                cmid = m.group(1)

            section_data["activities"].append({
                "type": mod_type,
                "cmid": cmid,
                "title": name,
                "url": urljoin(BASE, href),
            })

        if section_data["name"] or section_data["activities"]:
            course["sections"].append(section_data)

    return course


def dump_activity(act: dict) -> dict:
    """Дополнительно тянем страницу задания/ресурса (текст + файлы)"""
    if not act.get("url"):
        return act
    try:
        r = SESSION.get(act["url"], timeout=30)
        r.raise_for_status()
        soup = BeautifulSoup(r.text, "html.parser")

        # описание
        desc = soup.select_one(".activity-description, .box.generalbox, #intro, .contentwithoutlink")
        if desc:
            act["description"] = desc.get_text("\n", strip=True)[:3000]

        # файлы
        files = []
        for a in soup.select("a[href*='pluginfile.php'], a[href*='/mod/resource/'], a[href*='forcedownload=1']"):
            href = a.get("href")
            if href:
                files.append({
                    "name": a.get_text(strip=True) or "file",
                    "url": urljoin(BASE, href),
                })
        if files:
            act["files"] = files

        # дедлайн assign
        if act["type"] == "assign":
            for row in soup.select(".submissionstatustable tr, .activity-dates, .box.boxaligncenter"):
                text = row.get_text(" ", strip=True)
                if "Due" in text or "Срок" in text or "до" in text.lower():
                    act["deadline_raw"] = text[:300]
                    break
    except Exception as e:
        act["fetch_error"] = str(e)
    return act


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--user", required=True, help="email or username")
    ap.add_argument("--password", required=True)
    ap.add_argument("--out", default="spbftu_dump.json")
    ap.add_argument("--deep", action="store_true", help="также заходить внутрь каждого задания/ресурса")
    args = ap.parse_args()

    login(args.user, args.password)

    courses = get_my_courses()
    print(f"[+] Found {len(courses)} courses")

    result = []
    for c in courses:
        print(f"  → {c['title']} (id={c['id']})")
        full = parse_course(c["id"])
        if args.deep:
            for sec in full["sections"]:
                for i, act in enumerate(sec["activities"]):
                    if act["type"] in ("assign", "quiz", "resource", "page", "folder", "url"):
                        sec["activities"][i] = dump_activity(act)
        result.append(full)

    Path(args.out).write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"[+] Saved → {args.out}")


if __name__ == "__main__":
    main()