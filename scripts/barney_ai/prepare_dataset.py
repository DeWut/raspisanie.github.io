#!/usr/bin/env python3
"""Build a privacy-conscious SFT dataset from Barney's curated knowledge and approved examples.

Never export raw Telegram chats automatically. Only add examples to approved_dialogues.jsonl
after reviewing them and removing personal data, secrets, and copyrighted/private material.
"""
from __future__ import annotations
import argparse, json, re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SYSTEM = (
    "Ты Барни — спокойный, доброжелательный учебный помощник СПбГЛТУ. "
    "Отвечай естественно и по-русски, без канцелярита и лишних приветствий. "
    "Обычно отвечай кратко, но объясняй сложное по шагам и на примерах. "
    "Учитывай контекст диалога. Если вопрос неоднозначен — задай один уточняющий вопрос. "
    "Не выдумывай расписание, домашние задания, аудитории, преподавателей, сроки, "
    "пользовательские настройки или текущие события. Для таких данных используй инструменты приложения; "
    "если факта нет, честно скажи об этом. Не утверждай, что что-либо запомнил или сделал, если это не так."
)
SECRET_PATTERNS = [
    re.compile(r"\b\d{7,}:AA[A-Za-z0-9_-]{20,}\b"),
    re.compile(r"(?i)\b(api[_ -]?key|password|пароль|токен)\s*[:=]\s*\S+"),
    re.compile(r"\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b", re.I),
]
def safe(text: str) -> bool:
    return isinstance(text, str) and 1 <= len(text.strip()) <= 5000 and not any(p.search(text) for p in SECRET_PATTERNS)

def emit(out, user: str, assistant: str, seen: set[str]) -> bool:
    user, assistant = user.strip(), assistant.strip()
    if not safe(user) or not safe(assistant):
        return False
    key = re.sub(r"\W+", " ", user.lower()).strip()
    if key in seen:
        return False
    seen.add(key)
    row = {"messages": [
        {"role": "system", "content": SYSTEM},
        {"role": "user", "content": user},
        {"role": "assistant", "content": assistant},
    ]}
    out.write(json.dumps(row, ensure_ascii=False) + "\n")
    return True

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", default=str(ROOT / "data/barney_training/generated_sft.jsonl"))
    parser.add_argument("--approved", default=str(ROOT / "data/barney_training/approved_dialogues.jsonl"))
    parser.add_argument("--max-phrases-per-topic", type=int, default=4)
    args = parser.parse_args()
    knowledge = json.loads((ROOT / "backend/data/barney_knowledge_ru.json").read_text(encoding="utf-8"))
    seeds = ROOT / "data/barney_training/seed_dialogues.jsonl"
    seen: set[str] = set()
    count = 0
    Path(args.output).parent.mkdir(parents=True, exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as out:
        for entry in knowledge:
            answer = entry.get("answer", "")
            for phrase in entry.get("phrases", [])[:max(1, args.max_phrases_per_topic)]:
                count += emit(out, phrase, answer, seen)
        for source in (seeds, Path(args.approved)):
            if not source.exists():
                continue
            for line_no, line in enumerate(source.read_text(encoding="utf-8").splitlines(), 1):
                if not line.strip():
                    continue
                try:
                    row = json.loads(line)
                    if "messages" in row:
                        messages = row["messages"]
                        user = next(x["content"] for x in messages if x["role"] == "user")
                        assistant = next(x["content"] for x in reversed(messages) if x["role"] == "assistant")
                    else:
                        user, assistant = row["user"], row["assistant"]
                    count += emit(out, user, assistant, seen)
                except (ValueError, KeyError, StopIteration, TypeError) as exc:
                    raise SystemExit(f"Invalid {source}:{line_no}: {exc}")
    print(f"Prepared {count} unique examples -> {args.output}")
    print("Raw chat history was not imported. Review every approved example before training.")

if __name__ == "__main__":
    main()
