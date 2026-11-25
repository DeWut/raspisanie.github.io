#!/usr/bin/env python3
"""
Сборщик расписания СПбГЛТУ (eios.spbftu.ru) для всех групп.

Результат (то, что читает index.html):
    data/groups.json    - список групп [{"id": "19244", "name": "ЛПб-ИСИТ-26-1"}, ...]
    data/<id>.json      - расписание группы {"id","name","updated","lessons":{"1":[...], ...}}

Запуск:
    python update_schedule.py                 # найти группы автоматически и собрать всё
    python update_schedule.py 19244 19245     # только указанные ID групп
    python update_schedule.py --dump 19244    # сохранить сырой HTML в dump/ (для отладки парсера)
    python update_schedule.py --offline file.html   # разобрать локальный HTML, ничего не качая

Если у тебя есть data/groups.json, ID берутся и из него (удобно добавлять группы руками).
"""
import argparse
import json
import re
import sys
import time
from datetime import datetime
from pathlib import Path
from urllib.parse import urljoin

import requests
from bs4 import BeautifulSoup

BASE = "https://eios.spbftu.ru"
START_URL = BASE + "/Rasp/"
GROUP_URL = BASE + "/Rasp/Rasp.aspx?group={gid}&sem={sem}"
UA = "Mozilla/5.0 (Linux; Android 13) AppleWebKit/537.36 Chrome/120 Mobile Safari/537.36"

DATA_DIR = Path(__file__).parent / "data"
DUMP_DIR = Path(__file__).parent / "dump"

DAYS = ["Понедельник", "Вторник", "Среда", "Четверг", "Пятница", "Суббота", "Воскресенье"]
DAY_INDEX = {d.lower(): i + 1 for i, d in enumerate(DAYS)}

# Стандартная сетка пар (на случай, если в строке нет времени)
PAIR_TIMES = {
    1: ("09:15", "10:50"),
    2: ("11:10", "12:45"),
    3: ("13:30", "15:05"),
    4: ("15:15", "16:50"),
    5: ("17:00", "18:35"),
}

GROUP_LINK_RE = re.compile(r"[?&]group=(\d+)", re.I)
GROUP_NAME_RE = re.compile(
    r"[А-ЯЁA-Z][А-Яа-яЁёA-Za-z()]*(?:-[А-Яа-яЁёA-Za-z0-9()]+)*-\d{1,2}-\d+[А-Яа-яЁёA-Za-z]?"
)
TIME_RE = re.compile(r"(\d{1,2}):(\d{2})\D{1,12}?(\d{1,2}):(\d{2})")
PAIR_RE = re.compile(r"(\d+)\s*пара", re.I)
WEEK_VALUES = {"*": "*", "чёт": "чёт", "чет": "чёт", "нечёт": "нечёт", "нечет": "нечёт"}


def clean(s):
    return re.sub(r"[ \t\r\f\v\xa0]+", " ", s or "").strip()


def cell_lines(cell):
    """Текст ячейки построчно (<br> -> перевод строки)."""
    text = cell.get_text("\n", strip=True)
    return [clean(x) for x in text.split("\n") if clean(x)]


def hhmm(h, m):
    return f"{int(h):02d}:{int(m):02d}"


# ---------------------------------------------------------------- разбор страницы группы

def find_group_name(soup, fallback):
    text = clean(soup.get_text(" ", strip=True))
    m = GROUP_NAME_RE.search(text)
    if m:
        return m.group(0)
    if soup.title and soup.title.string:
        m = GROUP_NAME_RE.search(soup.title.string)
        if m:
            return m.group(0)
    return fallback


def pick_schedule_table(soup):
    """Выбираем таблицу, где больше всего «расписательных» признаков."""
    best, best_score = None, 0
    for table in soup.find_all("table"):
        low = table.get_text(" ", strip=True).lower()
        score = sum(low.count(k) for k in ("ауд.", "пара", "понедельник", "вторник", "среда", "четверг", "пятница"))
        # вложенные таблицы: берём самую «глубокую» с тем же счётом, чтобы не ловить обёртку страницы
        if score > best_score or (score == best_score and best is not None and best in table.find_all("table")):
            best, best_score = table, score
    return best


def row_day(tr):
    """Если строка - заголовок дня недели, вернуть номер дня 1..7."""
    text = clean(tr.get_text(" ", strip=True)).lower()
    if not text or len(text) > 60:
        return None
    for name, idx in DAY_INDEX.items():
        if text == name or text.startswith(name + " ") or text.startswith(name + ","):
            return idx
    return None


def parse_lesson_row(cells, state):
    """
    Разбор строки с занятием по содержимому ячеек, а не по номерам колонок:
    DevExpress добавляет отступы и rowspan, и индексы «плывут».
    Возвращает dict или None. state хранит последнюю виденную пару/время.
    """
    texts = [(c, cell_lines(c)) for c in cells]
    texts = [(c, ls) for c, ls in texts if ls]
    if not texts:
        return None

    # время / номер пары
    for c, ls in texts:
        joined = " ".join(ls)
        t = TIME_RE.search(joined)
        p = PAIR_RE.search(joined)
        if t:
            state["start"], state["end"] = hhmm(t.group(1), t.group(2)), hhmm(t.group(3), t.group(4))
            state["pair"] = int(p.group(1)) if p else None
            if state["pair"] is None:
                for n, (s, _) in PAIR_TIMES.items():
                    if s == state["start"]:
                        state["pair"] = n
            used_time = c
            break
    else:
        used_time = None
    if not state.get("start"):
        return None

    week, teacher, room, used = "*", "", "", {id(used_time)}

    # неделя
    for c, ls in texts:
        if id(c) in used:
            continue
        v = WEEK_VALUES.get(" ".join(ls).lower())
        if v:
            week = v
            used.add(id(c))
            break

    # преподаватель/аудитория (ячейка с "ауд")
    for c, ls in texts:
        if id(c) in used:
            continue
        if any("ауд" in x.lower() for x in ls):
            rooms = [x for x in ls if "ауд" in x.lower()]
            others = [x for x in ls if "ауд" not in x.lower()]
            room = ", ".join(rooms)
            teacher = ", ".join(others)
            used.add(id(c))
            break

    # предмет: самая длинная из оставшихся ячеек
    rest = [" ".join(ls) for c, ls in texts if id(c) not in used]
    rest = [r for r in rest if r not in ("*",)]
    if not rest:
        return None
    subject = max(rest, key=len)
    if not teacher and len(rest) > 1:            # аудитория могла быть без "ауд."
        others = [r for r in rest if r != subject]
        teacher = ", ".join(others)

    return {
        "pair": state["pair"] or 0,
        "start": state["start"],
        "end": state["end"],
        "name": subject,
        "teacher": teacher,
        "room": room,
        "week": week,
    }


def parse_group_html(html, gid):
    soup = BeautifulSoup(html, "html.parser")
    table = pick_schedule_table(soup)
    lessons = {}
    if table is not None:
        day, state = None, {}
        for tr in table.find_all("tr"):
            d = row_day(tr)
            if d:
                day, state = d, {}
                lessons.setdefault(str(day), [])
                continue
            if day is None:
                continue
            cells = tr.find_all("td", recursive=False) or tr.find_all("td")
            item = parse_lesson_row(cells, state)
            if item:
                lessons[str(day)].append(item)
    lessons = {k: v for k, v in lessons.items() if v}
    return {
        "id": str(gid),
        "name": find_group_name(soup, str(gid)),
        "updated": datetime.now().strftime("%Y-%m-%d %H:%M"),
        "lessons": lessons,
    }


# ---------------------------------------------------------------- список групп

def good_label(label, gid):
    return bool(label) and label != gid and not label.isdigit() and len(label) <= 60


def discover_groups(html, page_url):
    soup = BeautifulSoup(html, "html.parser")
    found = {}
    for a in soup.select("a[href]"):
        m = GROUP_LINK_RE.search(urljoin(page_url, a["href"]))
        if m:
            gid, label = m.group(1), clean(a.get_text(" ", strip=True))
            if good_label(label, gid) or gid not in found:
                found[gid] = label if good_label(label, gid) else ""
    for opt in soup.select("option[value]"):
        v, label = opt.get("value", "").strip(), clean(opt.get_text(" ", strip=True))
        if v.isdigit() and good_label(label, v):
            found[v] = label
    return found


def load_known_groups():
    p = DATA_DIR / "groups.json"
    if p.exists():
        try:
            return {g["id"]: g.get("name", g["id"]) for g in json.loads(p.read_text("utf-8"))}
        except Exception as e:
            print("groups.json не прочитан:", e)
    return {}


# ---------------------------------------------------------------- main

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ids", nargs="*", help="ID групп (если не заданы - автопоиск + groups.json)")
    ap.add_argument("--sem", default="1", help="параметр sem в URL (по умолчанию 1)")
    ap.add_argument("--delay", type=float, default=1.0, help="пауза между запросами, сек")
    ap.add_argument("--dump", action="store_true", help="сохранять сырой HTML в dump/")
    ap.add_argument("--offline", metavar="FILE", help="разобрать локальный HTML-файл и вывести JSON")
    args = ap.parse_args()

    if args.offline:
        html = Path(args.offline).read_text("utf-8", errors="replace")
        print(json.dumps(parse_group_html(html, "offline"), ensure_ascii=False, indent=2))
        return 0

    DATA_DIR.mkdir(exist_ok=True)
    s = requests.Session()
    s.headers.update({"User-Agent": UA, "Accept-Language": "ru-RU,ru;q=0.9"})

    known_groups = load_known_groups()
    # Explicit IDs mean exactly these groups, not every group in groups.json.
    groups = ({gid: known_groups.get(gid, gid) for gid in args.ids}
              if args.ids else known_groups)
    if not args.ids:
        try:
            r = s.get(START_URL, timeout=30)
            r.raise_for_status()
            if args.dump:
                DUMP_DIR.mkdir(exist_ok=True)
                (DUMP_DIR / "start.html").write_text(r.text, "utf-8")
            auto = discover_groups(r.text, r.url)
            print(f"Автопоиск на стартовой странице: найдено групп {len(auto)}")
            for k, v in auto.items():
                groups[k] = v or groups.get(k, k)
        except requests.RequestException as e:
            print("Стартовая страница недоступна:", e)
    if not groups:
        print("Нет ни одной группы. Добавь ID аргументами (python update_schedule.py 19244)\n"
              "или впиши их в data/groups.json.")
        return 1

    ok, failed = {}, []
    for n, gid in enumerate(sorted(groups), 1):
        url = GROUP_URL.format(gid=gid, sem=args.sem)
        try:
            r = s.get(url, timeout=30)
            r.raise_for_status()
            if args.dump:
                DUMP_DIR.mkdir(exist_ok=True)
                (DUMP_DIR / f"{gid}.html").write_text(r.text, "utf-8")
            data = parse_group_html(r.text, gid)
            total = sum(len(v) for v in data["lessons"].values())
            if total == 0:
                print(f"[{n}/{len(groups)}] {gid}: занятий не найдено - файл не перезаписан")
                failed.append(gid)
            else:
                if good_label(groups.get(gid, ""), gid):
                    data["name"] = groups[gid]
                (DATA_DIR / f"{gid}.json").write_text(json.dumps(data, ensure_ascii=False, indent=1), "utf-8")
                ok[gid] = data["name"]
                print(f"[{n}/{len(groups)}] {gid} {data['name']}: {total} занятий")
        except requests.RequestException as e:
            print(f"[{n}/{len(groups)}] {gid}: ошибка сети {e}")
            failed.append(gid)
        time.sleep(args.delay)

    # groups.json: только группы, для которых есть файл расписания
    existing = {p.stem for p in DATA_DIR.glob("*.json") if p.stem != "groups"}
    merged = []
    for gid in sorted(existing):
        try:
            name = ok.get(gid) or json.loads((DATA_DIR / f"{gid}.json").read_text("utf-8")).get("name", gid)
        except Exception:
            name = ok.get(gid, gid)
        merged.append({"id": gid, "name": name})
    merged.sort(key=lambda g: g["name"].lower())
    (DATA_DIR / "groups.json").write_text(json.dumps(merged, ensure_ascii=False, indent=1), "utf-8")

    print(f"\nГотово: сохранено {len(ok)}, не удалось {len(failed)}.")
    if failed:
        print("Проблемные ID:", ", ".join(failed), "\nЗапусти с --dump и пришли dump/<id>.html, подстрою парсер.")
    return 2 if failed else (0 if ok else 2)


if __name__ == "__main__":
    sys.exit(main())
