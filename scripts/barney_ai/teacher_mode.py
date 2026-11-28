#!/usr/bin/env python3
"""Human-reviewed synthetic dialogue generator using a local Ollama teacher model.

Nothing is sent to a cloud API. Examples are appended only after explicit approval.
"""
from __future__ import annotations
import json
import os
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OLLAMA_URL = os.environ.get("OLLAMA_HOST", "http://127.0.0.1:11434").rstrip("/")
MODEL = os.environ.get("BARNEY_TEACHER_MODEL", "lfm3")
OUTPUT = ROOT / "data/barney_training/approved_dialogues.jsonl"

SYSTEM = """Ты — редактор обучающих примеров для Барни, учебного помощника СПбГЛТУ.
Создавай реалистичные пары вопрос-ответ на русском языке.
Ответ должен быть точным, естественным, понятным, обычно 2–6 предложений.
Не выдумывай факты о реальном расписании, преподавателях, аудиториях, ДЗ, сроках,
студентах и внутренних функциях СПбГЛТУ. Если тема требует таких фактов, сформулируй
общий учебный пример без выдуманных реальных данных.
Не копируй персональные данные, пароли, токены или приватные переписки.
Верни только JSON-объект с двумя строковыми полями user и assistant."""

def generate(topic: str, index: int) -> dict[str, str]:
    prompt = (
        f"Создай обучающий диалог #{index} по теме: {topic}. "
        "Сделай вопрос непохожим на типовые формулировки и ответь как Барни. "
        "Не упоминай, что ты языковая модель. Верни JSON: "
        '{"user":"...","assistant":"..."}'
    )
    body = json.dumps({
        "model": MODEL,
        "stream": False,
        "format": "json",
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": prompt},
        ],
        "options": {"temperature": 0.65, "top_p": 0.9},
    }).encode("utf-8")
    req = urllib.request.Request(
        f"{OLLAMA_URL}/api/chat", data=body,
        headers={"Content-Type": "application/json"}, method="POST"
    )
    with urllib.request.urlopen(req, timeout=180) as response:
        payload = json.loads(response.read().decode("utf-8"))
    content = payload.get("message", {}).get("content", "")
    item = json.loads(content)
    user, assistant = item.get("user"), item.get("assistant")
    if not isinstance(user, str) or not isinstance(assistant, str):
        raise ValueError("Ollama returned no user/assistant strings")
    user, assistant = user.strip(), assistant.strip()
    if not user or not assistant or len(user) > 1000 or len(assistant) > 5000:
        raise ValueError("Generated example is empty or too long")
    return {"user": user, "assistant": assistant}

def main() -> None:
    print("=" * 68)
    print(" BARNEY AI | LOCAL TEACHER MODE")
    print(f" Ollama: {OLLAMA_URL} | model: {MODEL}")
    print(" Human approval required. No Telegram history is imported.")
    print("=" * 68)
    topic = input("Тема для новых диалогов: ").strip()
    if not topic:
        print("Тема не указана.")
        return
    try:
        count = int(input("Сколько примеров предложить (1-30): ").strip())
    except ValueError:
        print("Нужно ввести число.")
        return
    if not 1 <= count <= 30:
        print("Допустимо от 1 до 30.")
        return

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    approved = 0
    for i in range(1, count + 1):
        print(f"\n[{i}/{count}] Запрашиваю локального учителя...")
        try:
            item = generate(topic, i)
        except Exception as exc:
            print(f"[ERROR] Не удалось получить пример: {exc}")
            print("Проверь, что Ollama запущена и модель доступна: ollama list")
            break
        print("\nВОПРОС:")
        print(item["user"])
        print("\nОТВЕТ:")
        print(item["assistant"])
        choice = input("\n[ Y ] принять  [ E ] изменить ответ  [ N ] отклонить  [ Q ] выход: ").strip().lower()
        if choice == "q":
            break
        if choice == "e":
            edited = input("Введи исправленный ответ: ").strip()
            if edited:
                item["assistant"] = edited
                choice = "y"
        if choice == "y":
            with OUTPUT.open("a", encoding="utf-8") as file:
                file.write(json.dumps(item, ensure_ascii=False) + "\n")
            approved += 1
            print("[SAVED] Добавлено в approved_dialogues.jsonl")
        else:
            print("[SKIP] Пример не сохранён.")
    print(f"\nГотово. Одобрено: {approved}. Файл: {OUTPUT}")
    print("Следующий шаг: запусти BARNEY_AI.bat -> [3] Prepare dataset.")

if __name__ == "__main__":
    main()
