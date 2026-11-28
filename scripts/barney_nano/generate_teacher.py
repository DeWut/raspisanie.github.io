#!/usr/bin/env python3
"""Generate candidate university-focused conversations from a local Ollama teacher."""
from __future__ import annotations
import argparse, json, os, random, time, urllib.error, urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT = ROOT / "data" / "barney_nano" / "teacher_raw.jsonl"
TOPICS = [
    "Университетское расписание: пары, окна, переносы, аудитории и дедлайны. Не выдумывай реальные данные.",
    "Объяснение сложной темы студенту вуза: сначала интуиция, затем точное объяснение и пример.",
    "Программирование на Python и Rust, алгоритмы, отладка, базы данных и API.",
    "Подготовка курсовой, научного доклада, лабораторной или исследовательского проекта.",
    "Организация студенческой жизни: учебные группы, мероприятия, объявления и коммуникация.",
    "Как пользоваться цифровым кабинетом университетского приложения и безопасно работать с данными.",
    "Математика, статистика, дискретная математика и основы инженерных дисциплин.",
    "Академическая добросовестность, цитирование, проверка источников и самостоятельная работа.",
    "Планирование сессии, подготовка к экзамену и управление учебной нагрузкой.",
    "Корректный ответ, когда у ассистента нет доступа к расписанию, оценкам или данным конкретного вуза.",
]
SYSTEM = """Ты — LFM3, учитель для подготовки компактной русскоязычной модели BARNEY-NANO.
Создай ОДИН качественный пример диалога для университетского помощника.
Ориентируйся на студентов и сотрудников вузов, НЕ на школьников.
Ответ должен быть естественным, точным, полезным и не чрезмерно длинным.
Не выдумывай конкретные расписания, оценки, ФИО, аудитории, правила или события.
Если нужны актуальные данные из приложения, объясни, что их нужно получить через backend.
Не выполняй опасные действия и не проси пароли, токены или персональные данные.
Верни только JSON вида:
{"messages":[{"role":"user","content":"..."},{"role":"assistant","content":"..."}]}
Никакого Markdown и текста вне JSON."""

def ollama_chat(base_url: str, model: str, topic: str, timeout: int) -> dict:
    payload = {"model": model, "stream": False, "format": "json",
        "options": {"temperature": 0.75, "top_p": 0.9, "num_predict": 700},
        "messages": [{"role": "system", "content": SYSTEM},
                     {"role": "user", "content": "Тема примера: " + topic + "\nСформируй один диалог."}]}
    request = urllib.request.Request(base_url.rstrip("/") + "/api/chat",
        data=json.dumps(payload, ensure_ascii=False).encode("utf-8"),
        headers={"Content-Type": "application/json"}, method="POST")
    with urllib.request.urlopen(request, timeout=timeout) as response:
        outer = json.loads(response.read().decode("utf-8"))
    item = json.loads(outer.get("message", {}).get("content", ""))
    messages = item.get("messages")
    if not isinstance(messages, list) or len(messages) < 2:
        raise ValueError("Teacher response has no valid messages array")
    if messages[0].get("role") != "user" or messages[-1].get("role") != "assistant":
        raise ValueError("Expected user message followed by assistant answer")
    if not all(isinstance(m.get("content"), str) and m["content"].strip() for m in messages):
        raise ValueError("Empty or invalid message content")
    return {"messages": messages, "teacher": model, "topic": topic}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--count", type=int, default=20)
    parser.add_argument("--model", default=os.getenv("BARNEY_TEACHER_MODEL", "lfm3"))
    parser.add_argument("--base-url", default=os.getenv("OLLAMA_HOST", "http://127.0.0.1:11434"))
    parser.add_argument("--output", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--timeout", type=int, default=180)
    args = parser.parse_args()
    if args.count < 1 or args.count > 500:
        raise SystemExit("--count must be between 1 and 500")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    created = 0
    with args.output.open("a", encoding="utf-8") as out:
        for index in range(args.count):
            topic = random.choice(TOPICS)
            try:
                item = ollama_chat(args.base_url, args.model, topic, args.timeout)
                out.write(json.dumps(item, ensure_ascii=False) + "\n")
                out.flush()
                created += 1
                print(f"[{index + 1}/{args.count}] candidate saved: {topic[:76]}")
            except (urllib.error.URLError, TimeoutError, ValueError, json.JSONDecodeError) as exc:
                print(f"[{index + 1}/{args.count}] generation failed: {exc}")
            time.sleep(0.2)
    print(f"Generated {created} candidates. Review them before training: {args.output}")

if __name__ == "__main__":
    main()
