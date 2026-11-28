#!/usr/bin/env python3
"""BARNEY-NANO model reporting, human QA review and safe Ollama candidate creation."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
TRAINING = ROOT / "training"
DATA = ROOT / "data" / "barney_nano"
ADAPTER = TRAINING / "barney-nano-adapter"
APPROVED = DATA / "approved.jsonl"
MERGED = TRAINING / "barney-nano-merged"
BENCHMARK_JSON = TRAINING / "barney-nano-benchmark.json"
BENCHMARK_TEXT = TRAINING / "barney-nano-benchmark.txt"
QA_FILE = TRAINING / "barney-nano-qa.json"
READINESS_JSON = TRAINING / "barney-nano-readiness.json"
READINESS_MD = TRAINING / "barney-nano-readiness.md"
CANDIDATE_POINTER = TRAINING / "barney-nano-candidate.json"
DEFAULT_MODEL = "HuggingFaceTB/SmolLM2-360M-Instruct"
PRODUCTION_MIN_EXAMPLES = 300


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def read_json(path: Path) -> dict[str, Any] | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
        return value if isinstance(value, dict) else None
    except (OSError, json.JSONDecodeError):
        return None


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def format_bytes(size: int | None) -> str:
    if size is None:
        return "н/д"
    value = float(size)
    for unit in ("Б", "КиБ", "МиБ", "ГиБ", "ТиБ"):
        if value < 1024 or unit == "ТиБ":
            return f"{value:,.2f} {unit}".replace(",", " ")
        value /= 1024
    return f"{size} Б"


def directory_size(path: Path) -> int | None:
    if not path.exists():
        return None
    total = 0
    for item in path.rglob("*"):
        try:
            if item.is_file():
                total += item.stat().st_size
        except OSError:
            continue
    return total


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def adapter_weights_hash(adapter_dir: Path) -> str | None:
    files = sorted(
        p for p in adapter_dir.glob("adapter_model*")
        if p.is_file() and p.suffix in {".safetensors", ".bin"}
    )
    if not files:
        return None
    digest = hashlib.sha256()
    for path in files:
        digest.update(path.name.encode("utf-8"))
        digest.update(bytes.fromhex(sha256_file(path)))
    return digest.hexdigest()


def dataset_stats(path: Path) -> dict[str, Any]:
    result = {"examples": 0, "unique_user_prompts": 0, "invalid_lines": 0, "sha256": None}
    if not path.is_file():
        return result
    prompts: set[str] = set()
    digest = hashlib.sha256()
    with path.open("rb") as raw:
        for chunk in iter(lambda: raw.read(1024 * 1024), b""):
            digest.update(chunk)
    result["sha256"] = digest.hexdigest()
    with path.open("r", encoding="utf-8") as stream:
        for line in stream:
            if not line.strip():
                continue
            try:
                item = json.loads(line)
                messages = item.get("messages", [])
                user = next((m.get("content", "") for m in messages if m.get("role") == "user"), "")
                assistant = next((m.get("content", "") for m in reversed(messages) if m.get("role") == "assistant"), "")
                if not user.strip() or not assistant.strip():
                    result["invalid_lines"] += 1
                    continue
                result["examples"] += 1
                prompts.add(" ".join(user.casefold().split()))
            except (json.JSONDecodeError, AttributeError, TypeError):
                result["invalid_lines"] += 1
    result["unique_user_prompts"] = len(prompts)
    return result


def find_trainer_state(adapter_dir: Path) -> dict[str, Any] | None:
    candidates = []
    root_state = adapter_dir / "trainer_state.json"
    if root_state.exists():
        candidates.append(root_state)
    candidates.extend(adapter_dir.glob("checkpoint-*/trainer_state.json"))
    states = []
    for path in candidates:
        state = read_json(path)
        if state:
            try:
                mtime = path.stat().st_mtime
            except OSError:
                mtime = 0
            states.append((mtime, path, state))
    if not states:
        return None
    _, path, state = max(states, key=lambda x: x[0])
    history = state.get("log_history", [])
    eval_values = []
    train_values = []
    for row in history if isinstance(history, list) else []:
        if not isinstance(row, dict):
            continue
        try:
            if "eval_loss" in row and math.isfinite(float(row["eval_loss"])):
                eval_values.append(float(row["eval_loss"]))
            if "loss" in row and math.isfinite(float(row["loss"])):
                train_values.append(float(row["loss"]))
        except (TypeError, ValueError):
            continue
    return {
        "path": str(path),
        "best_metric": state.get("best_metric"),
        "best_model_checkpoint": state.get("best_model_checkpoint"),
        "global_step": state.get("global_step"),
        "epoch": state.get("epoch"),
        "min_eval_loss": min(eval_values) if eval_values else None,
        "last_eval_loss": eval_values[-1] if eval_values else None,
        "last_train_loss": train_values[-1] if train_values else None,
        "eval_records": len(eval_values),
    }


def count_adapter_parameters(adapter_dir: Path) -> int | None:
    files = list(adapter_dir.glob("adapter_model*.safetensors"))
    if files:
        try:
            from safetensors import safe_open
            count = 0
            for path in files:
                with safe_open(str(path), framework="pt", device="cpu") as handle:
                    for key in handle.keys():
                        shape = handle.get_slice(key).get_shape()
                        count += math.prod(shape)
            return count
        except Exception as exc:
            print(f"[WARN] Не удалось посчитать параметры safetensors: {exc}")
            return None
    binary = adapter_dir / "adapter_model.bin"
    if binary.exists():
        try:
            import torch
            state = torch.load(binary, map_location="cpu", weights_only=True)
            return sum(value.numel() for value in state.values() if hasattr(value, "numel"))
        except Exception as exc:
            print(f"[WARN] Не удалось прочитать adapter_model.bin: {exc}")
    return None


def count_base_parameters(model_id: str) -> int | None:
    try:
        import torch
        from transformers import AutoConfig, AutoModelForCausalLM
        config = AutoConfig.from_pretrained(model_id, local_files_only=True)
        with torch.device("meta"):
            model = AutoModelForCausalLM.from_config(config)
        count = sum(parameter.numel() for parameter in model.parameters())
        del model
        return int(count)
    except Exception as exc:
        print(f"[WARN] Не удалось получить число параметров базовой модели: {exc}")
        return None


def benchmark_stats(path: Path) -> dict[str, Any]:
    payload = read_json(path) if path.exists() else None
    if not payload:
        return {"exists": False, "cases": 0, "non_empty": 0, "payload": None}
    cases = payload.get("cases", [])
    non_empty = sum(
        1 for row in cases
        if isinstance(row, dict) and str(row.get("candidate_answer", "")).strip()
    )
    return {
        "exists": True,
        "cases": len(cases) if isinstance(cases, list) else 0,
        "non_empty": non_empty,
        "payload": payload,
    }


def load_candidate() -> dict[str, Any] | None:
    pointer = read_json(CANDIDATE_POINTER)
    if not pointer:
        return None
    manifest_path = ROOT / pointer.get("manifest", "")
    manifest = read_json(manifest_path) if pointer.get("manifest") else None
    if not manifest:
        return None
    return manifest


def build_report(model_id: str = DEFAULT_MODEL) -> dict[str, Any]:
    adapter_exists = (ADAPTER / "adapter_config.json").is_file() and any(
        (ADAPTER / name).is_file() for name in ("adapter_model.safetensors", "adapter_model.bin")
    )
    data = dataset_stats(APPROVED)
    trainer = find_trainer_state(ADAPTER)
    benchmark = benchmark_stats(BENCHMARK_JSON)
    qa = read_json(QA_FILE)
    candidate = load_candidate()

    base_params = count_base_parameters(model_id) if adapter_exists else None
    trainable_params = count_adapter_parameters(ADAPTER) if adapter_exists else None
    total_with_adapter = (base_params + trainable_params) if base_params is not None and trainable_params is not None else None
    trainable_pct = (trainable_params / total_with_adapter * 100) if total_with_adapter else None
    base_size = None
    try:
        from huggingface_hub import snapshot_download
        cached = Path(snapshot_download(repo_id=model_id, local_files_only=True))
        base_size = directory_size(cached)
    except Exception:
        cached = None

    adapter_size = directory_size(ADAPTER)
    adapter_weights_size = sum(
        p.stat().st_size for p in ADAPTER.glob("adapter_model*") if p.is_file()
    ) if ADAPTER.exists() else 0
    adapter_hash = adapter_weights_hash(ADAPTER) if adapter_exists else None
    benchmark_hash = sha256_file(BENCHMARK_JSON) if BENCHMARK_JSON.is_file() else None
    candidate_merged = (ROOT / candidate.get("merged_checkpoint", "")) if candidate else None
    merged_size = directory_size(candidate_merged) if candidate_merged and candidate_merged.exists() else directory_size(MERGED)
    candidate_size = candidate.get("ollama_model_size_bytes") if candidate else None
    adapter_to_base_size_pct = (
        adapter_weights_size / base_size * 100
        if base_size and adapter_weights_size else None
    )

    qa_cases = qa.get("cases", []) if qa else []
    qa_pass = bool(
        qa and qa.get("status") == "passed"
        and qa.get("benchmark_sha256") == benchmark_hash
        and isinstance(qa_cases, list)
        and len(qa_cases) >= 6
        and len(qa_cases) == benchmark["cases"]
        and all(isinstance(row, dict) and row.get("passed") is True for row in qa_cases)
    )
    benchmark_ready = bool(
        benchmark["exists"] and benchmark["cases"] >= 6
        and benchmark["non_empty"] == benchmark["cases"]
    )
    eval_ready = bool(trainer and trainer.get("min_eval_loss") is not None)
    dataset_production_ready = data["examples"] >= PRODUCTION_MIN_EXAMPLES
    candidate_ready = bool(
        candidate
        and candidate.get("local_smoke_test_passed") is True
        and candidate.get("dataset_sha256") == data["sha256"]
        and candidate.get("adapter_weights_sha256") == adapter_hash
        and candidate.get("benchmark_sha256") == benchmark_hash
        and candidate.get("qa_benchmark_sha256") == (qa.get("benchmark_sha256") if qa else None)
        and qa_pass
    )

    gates = [
        {"id": "adapter", "label": "Обученный LoRA-адаптер сохранён", "passed": adapter_exists},
        {"id": "dataset", "label": f"Не менее {PRODUCTION_MIN_EXAMPLES} проверенных примеров", "passed": dataset_production_ready},
        {"id": "eval", "label": "Есть eval_loss из обучающей проверки", "passed": eval_ready},
        {"id": "benchmark", "label": "Создан benchmark: базовая и обученная модели", "passed": benchmark_ready},
        {"id": "human_qa", "label": "Ручная проверка ответов пройдена", "passed": qa_pass},
        {"id": "ollama_candidate", "label": "Кандидат импортирован и отвечает через Ollama", "passed": candidate_ready},
    ]

    if not adapter_exists:
        status = "NOT_TRAINED"
        summary = "Адаптер не найден. Сначала завершить обучение."
    elif not benchmark_ready or not eval_ready:
        status = "NEEDS_EVALUATION"
        summary = "Обучение найдено, но недостаточно технических результатов. Запустить benchmark."
    elif not qa_pass:
        status = "NEEDS_HUMAN_REVIEW"
        summary = "Нужна ручная проверка ответов. Пока не публиковать."
    elif not dataset_production_ready:
        status = "PILOT_ONLY"
        summary = f"Можно тестировать локально, но для публикации нужно минимум {PRODUCTION_MIN_EXAMPLES} одобренных примеров по правилам проекта."
    elif not candidate_ready:
        status = "NEEDS_LOCAL_CANDIDATE"
        summary = "Качество предварительно проверено. Создать и протестировать отдельную модель Ollama."
    else:
        status = "READY_FOR_PROMOTION"
        summary = "Все локальные ворота пройдены. Можно рассматривать переключение backend после удалённого preflight."

    gates_passed = sum(1 for gate in gates if gate["passed"])
    readiness_percent = round(gates_passed / len(gates) * 100, 1) if gates else 0.0

    report = {
        "schema_version": 1,
        "generated_at": now_iso(),
        "status": status,
        "readiness_checklist": {
            "passed": gates_passed,
            "total": len(gates),
            "percent": readiness_percent,
        },
        "summary": summary,
        "model_id": model_id,
        "production_min_approved_examples": PRODUCTION_MIN_EXAMPLES,
        "dataset": data,
        "parameters": {
            "base_total": base_params,
            "adapter_trainable": trainable_params,
            "total_with_adapter": total_with_adapter,
            "trainable_percent": round(trainable_pct, 5) if trainable_pct is not None else None,
        },
        "sizes_bytes": {
            "base_model_cache": base_size,
            "adapter_directory": adapter_size,
            "adapter_weights": adapter_weights_size,
            "merged_checkpoint": merged_size,
            "ollama_candidate": candidate_size,
        },
        "size_ratios": {
            "adapter_weights_percent_of_base_cache": round(adapter_to_base_size_pct, 5) if adapter_to_base_size_pct is not None else None,
        },
        "artifact_hashes": {
            "approved_dataset_sha256": data["sha256"],
            "adapter_weights_sha256": adapter_hash,
            "benchmark_sha256": benchmark_hash,
            "qa_benchmark_sha256": qa.get("benchmark_sha256") if qa else None,
        },
        "training": trainer,
        "benchmark": {
            "exists": benchmark["exists"],
            "cases": benchmark["cases"],
            "non_empty_candidate_answers": benchmark["non_empty"],
        },
        "human_review": {
            "status": qa.get("status") if qa else "not_reviewed",
            "reviewed_at": qa.get("reviewed_at") if qa else None,
            "passed_cases": sum(1 for row in qa.get("cases", []) if isinstance(row, dict) and row.get("passed") is True) if qa else 0,
            "total_cases": len(qa.get("cases", [])) if qa else 0,
            "matches_current_benchmark": bool(qa and qa.get("benchmark_sha256") == benchmark_hash),
        },
        "candidate": {
            "tag": candidate.get("tag") if candidate else None,
            "local_smoke_test_passed": candidate_ready,
            "release_id": candidate.get("release_id") if candidate else None,
        },
        "gates": gates,
        "readiness_note": "Количество пройденных ворот — прогресс по чек-листу, а не научно измеренный процент качества модели.",
    }
    write_json(READINESS_JSON, report)

    lines = [
        "# BARNEY-NANO — отчёт о готовности",
        "",
        f"- Сформирован: {report['generated_at']}",
        f"- Статус: **{status}**",
        f"- {summary}",
        "",
        "## Параметры и место на диске",
        "",
        f"- Базовая модель: {base_params:,} параметров" if base_params is not None else "- Базовая модель: число параметров не определено",
        f"- Обучаемые параметры LoRA: {trainable_params:,}" if trainable_params is not None else "- Обучаемые параметры LoRA: не определены",
        f"- Доля обучаемых параметров: {trainable_pct:.5f}%" if trainable_pct is not None else "- Доля обучаемых параметров: не определена",
        f"- Кеш базовой модели: {format_bytes(base_size)}",
        f"- Файлы весов адаптера: {format_bytes(adapter_weights_size)}",
        f"- Вся папка адаптера/checkpoints: {format_bytes(adapter_size)}",
        f"- Весовой файл адаптера составляет {adapter_to_base_size_pct:.5f}% от кеша базовой модели" if adapter_to_base_size_pct is not None else "- Доля размера адаптера от базовой модели: не определена",
        f"- Объединённая HF-модель: {format_bytes(merged_size)}",
        f"- Модель Ollama-кандидат: {format_bytes(candidate_size)}",
        "",
        "## Обучение и качество",
        "",
        f"- Проверенных примеров: {data['examples']}",
        f"- Уникальных пользовательских запросов: {data['unique_user_prompts']}",
        f"- Некорректных строк в датасете: {data['invalid_lines']}",
        f"- Лучший eval_loss: {trainer.get('min_eval_loss')}" if trainer and trainer.get("min_eval_loss") is not None else "- eval_loss: отсутствует",
        f"- Benchmark: {benchmark['non_empty']}/{benchmark['cases']} непустых ответов" if benchmark["exists"] else "- Benchmark: ещё не создан",
        f"- Ручная проверка: {report['human_review']['passed_cases']}/{report['human_review']['total_cases']} пройдено",
        "",
        "## Ворота выпуска",
        "",
    ]
    lines.extend(
        f"- [{'x' if row['passed'] else ' '}] {row['label']}"
        for row in gates
    )
    lines += ["", report["readiness_note"], ""]
    READINESS_MD.parent.mkdir(parents=True, exist_ok=True)
    READINESS_MD.write_text("\n".join(lines), encoding="utf-8")
    return report


def print_report(report: dict[str, Any]) -> None:
    params = report["parameters"]
    sizes = report["sizes_bytes"]
    dataset = report["dataset"]
    training = report.get("training") or {}
    print("\n" + "=" * 68)
    print(" BARNEY-NANO | ОТЧЁТ О ГОТОВНОСТИ")
    print("=" * 68)
    print(f"Статус: {report['status']}")
    print(report["summary"])
    print("\nПАРАМЕТРЫ")
    if params["base_total"] is not None:
        print(f"База:              {params['base_total']:,}")
    if params["adapter_trainable"] is not None:
        print(f"Обучено через LoRA:{params['adapter_trainable']:>15,}")
    if params["trainable_percent"] is not None:
        print(f"Доля обучаемых:    {params['trainable_percent']:.5f}%")
    print("\nРАЗМЕРЫ")
    for title, key in [
        ("Базовая модель", "base_model_cache"),
        ("Веса адаптера", "adapter_weights"),
        ("Адаптер + checkpoints", "adapter_directory"),
        ("Объединённая модель", "merged_checkpoint"),
        ("Ollama candidate", "ollama_candidate"),
    ]:
        print(f"{title:<25} {format_bytes(sizes.get(key))}")
    print("\nДАННЫЕ И МЕТРИКИ")
    if report["size_ratios"]["adapter_weights_percent_of_base_cache"] is not None:
        print(f"Вес адаптера / кеш базы: {report['size_ratios']['adapter_weights_percent_of_base_cache']:.5f}%")
    print(f"Одобренные примеры: {dataset['examples']} ({dataset['unique_user_prompts']} уникальных запросов)")
    print(f"Eval loss: {training.get('min_eval_loss', 'нет данных')}")
    print(f"Benchmark: {report['benchmark']['non_empty_candidate_answers']}/{report['benchmark']['cases']}")
    print(f"Ручная проверка: {report['human_review']['passed_cases']}/{report['human_review']['total_cases']}")
    print("\nВОРОТА ВЫПУСКА")
    passed = sum(1 for gate in report["gates"] if gate["passed"])
    for gate in report["gates"]:
        print(f" [{'OK' if gate['passed'] else '  '}] {gate['label']}")
    readiness_percent = report["readiness_checklist"]["percent"]
    print(f"\nПройдено: {passed}/{len(report['gates'])} контрольных пунктов ({readiness_percent:.1f}%).")
    print("Это прогресс чек-листа выпуска, а не процент точности или качества ответов.")
    print(f"\nОтчёт JSON: {READINESS_JSON}")
    print(f"Отчёт Markdown: {READINESS_MD}")


def review_benchmark() -> int:
    stats = benchmark_stats(BENCHMARK_JSON)
    if not stats["exists"] or stats["cases"] < 6:
        print("[ERROR] Сначала выполни пункт [6] Evaluate trained adapter.")
        return 2
    cases = stats["payload"].get("cases", [])
    if not cases:
        print("[ERROR] В benchmark нет тестовых случаев.")
        return 2

    print("\nПроверяй именно ответы BARNEY-NANO, а не ответы базовой модели.")
    print("Для каждого случая оцени: точность, русский язык, отсутствие выдуманных данных и безопасное поведение.")
    print("Особое внимание удели тесту с просьбой сообщить пароль.\n")
    decisions = []
    for index, row in enumerate(cases, 1):
        print("\n" + "=" * 76)
        print(f"СЛУЧАЙ {index}/{len(cases)}")
        print("\nВОПРОС:\n" + str(row.get("prompt", "")))
        print("\nОТВЕТ BARNEY-NANO:\n" + str(row.get("candidate_answer", "")))
        if row.get("base_answer"):
            print("\nОТВЕТ БАЗОВОЙ МОДЕЛИ:\n" + str(row.get("base_answer", "")))
        choice = input("\nОтвет подходит для выпуска? [Y] да / [N] нет: ").strip().lower()
        decisions.append({
            "case_id": row.get("case_id", index),
            "passed": choice == "y",
            "checks": ["correctness", "russian_fluency", "groundedness", "safety"],
        })

    passed = all(row["passed"] for row in decisions)
    payload = {
        "schema_version": 1,
        "reviewed_at": now_iso(),
        "benchmark_file": str(BENCHMARK_JSON.relative_to(ROOT)),
        "benchmark_sha256": sha256_file(BENCHMARK_JSON),
        "status": "passed" if passed else "failed",
        "cases": decisions,
        "note": "Manual reviewer decision. Re-run after any new training or benchmark generation.",
    }
    write_json(QA_FILE, payload)
    print(f"\nРучная проверка: {'ПРОЙДЕНА' if passed else 'НЕ ПРОЙДЕНА'} ({sum(x['passed'] for x in decisions)}/{len(decisions)}).")
    print(f"Результат сохранён: {QA_FILE}")
    report = build_report()
    print_report(report)
    return 0 if passed else 1


def local_ollama_chat(tag: str, prompt: str) -> tuple[bool, str]:
    import requests
    base = os.getenv("OLLAMA_HOST", "http://127.0.0.1:11434").rstrip("/")
    response = requests.post(
        base + "/api/chat",
        json={
            "model": tag,
            "stream": False,
            "messages": [
                {"role": "system", "content": "Ты Барни — компактный университетский помощник. Отвечай по-русски, не выдумывай факты и не проси пароли или секреты."},
                {"role": "user", "content": prompt},
            ],
            "options": {"temperature": 0.1, "num_predict": 96, "num_ctx": 2048},
        },
        timeout=180,
    )
    response.raise_for_status()
    answer = str(response.json().get("message", {}).get("content", "")).strip()
    return bool(answer), answer


def prepare_candidate() -> int:
    if not (ADAPTER / "adapter_config.json").is_file():
        print(f"[ERROR] Не найден адаптер: {ADAPTER}")
        return 2
    if not BENCHMARK_JSON.is_file():
        print("[ERROR] Сначала выполни пункт [6] Evaluate trained adapter.")
        return 2
    qa = read_json(QA_FILE)
    current_benchmark_hash = sha256_file(BENCHMARK_JSON)
    benchmark = benchmark_stats(BENCHMARK_JSON)
    if (
        not qa
        or qa.get("status") != "passed"
        or qa.get("benchmark_sha256") != current_benchmark_hash
        or benchmark["cases"] < 6
        or not isinstance(qa.get("cases"), list)
        or len(qa["cases"]) != benchmark["cases"]
        or not all(isinstance(row, dict) and row.get("passed") is True for row in qa["cases"])
    ):
        print("[BLOCKED] Ручная проверка отсутствует, устарела или benchmark не пройден целиком. Запусти [10] повторно.")
        return 3

    data = dataset_stats(APPROVED)
    release_id = datetime.now().strftime("rc-%Y%m%d-%H%M%S")
    release_dir = TRAINING / "releases" / release_id
    merged_dir = release_dir / "merged"
    release_dir.mkdir(parents=True, exist_ok=False)

    merge_script = ROOT / "scripts" / "barney_nano" / "merge_adapter.py"
    print("\n[1/3] Создание отдельного merged checkpoint для этого релиза...")
    result = subprocess.run(
        [
            sys.executable, str(merge_script),
            "--adapter", str(ADAPTER),
            "--output", str(merged_dir),
        ],
        cwd=ROOT,
    )
    if result.returncode != 0 or not (merged_dir / "config.json").is_file():
        print("[ERROR] Merge не завершился успешно. Текущая модель Ollama не затрагивалась.")
        return 4

    safe_release_id = release_id.replace("-", "")
    tag = f"barney-nano:{safe_release_id}"
    modelfile = release_dir / "Modelfile"
    merged_from = merged_dir.resolve().as_posix()
    modelfile.write_text(
        f'FROM "{merged_from}"\n'
        "PARAMETER temperature 0.35\n"
        "PARAMETER top_p 0.9\n"
        "PARAMETER repeat_penalty 1.1\n"
        'SYSTEM """Ты Барни — спокойный и доброжелательный университетский помощник. '
        'Отвечай по-русски, не выдумывай актуальные данные расписания и не проси пароли или секреты."""\n',
        encoding="utf-8",
    )

    ollama = shutil.which("ollama")
    if not ollama:
        print("[ERROR] Команда ollama не найдена. Текущая модель не затронута.")
        return 5
    print("\n[2/3] Импорт отдельной версии в Ollama:")
    print(f"Тег кандидата: {tag}")
    result = subprocess.run([ollama, "create", tag, "-f", str(modelfile)], cwd=release_dir)
    if result.returncode != 0:
        print("[ERROR] Ollama не смог импортировать этот Safetensors checkpoint.")
        print("Текущая модель и тег lfm3 не изменялись. Проверьте поддержку архитектуры в Ollama.")
        return 6

    print("\n[3/3] Локальный smoke test через Ollama API...")
    smoke_prompt = "Кратко объясни первокурснику, чем семинар отличается от лекции."
    try:
        ok, answer = local_ollama_chat(tag, smoke_prompt)
    except Exception as exc:
        ok, answer = False, str(exc)
    if not ok:
        print("[ERROR] Модель создана, но локальный запрос не прошёл.")
        print(str(answer)[:500])
        print("Кандидат оставлен отдельным тегом; production не переключался.")
        return 7

    ollama_size = None
    try:
        import requests
        base = os.getenv("OLLAMA_HOST", "http://127.0.0.1:11434").rstrip("/")
        tags = requests.get(base + "/api/tags", timeout=10).json().get("models", [])
        found = next((row for row in tags if row.get("name") == tag), None)
        ollama_size = found.get("size") if found else None
    except Exception:
        pass

    report = build_report()
    manifest = {
        "schema_version": 1,
        "release_id": release_id,
        "created_at": now_iso(),
        "tag": tag,
        "base_model": DEFAULT_MODEL,
        "merged_checkpoint": str(merged_dir.relative_to(ROOT)),
        "modelfile": str(modelfile.relative_to(ROOT)),
        "approved_examples": data["examples"],
        "production_eligible": data["examples"] >= PRODUCTION_MIN_EXAMPLES,
        "dataset_sha256": data["sha256"],
        "adapter_weights_sha256": adapter_weights_hash(ADAPTER),
        "benchmark_sha256": sha256_file(BENCHMARK_JSON),
        "qa_benchmark_sha256": qa.get("benchmark_sha256"),
        "manual_qa_status": qa.get("status"),
        "local_smoke_test_passed": True,
        "local_smoke_prompt": smoke_prompt,
        "local_smoke_answer": answer,
        "ollama_model_size_bytes": ollama_size,
        "readiness_at_build": report.get("status"),
        "note": "Versioned candidate. Existing production model tag was not overwritten.",
    }
    manifest_path = release_dir / "manifest.json"
    write_json(manifest_path, manifest)
    write_json(CANDIDATE_POINTER, {
        "tag": tag,
        "release_id": release_id,
        "manifest": str(manifest_path.relative_to(ROOT)),
        "created_at": manifest["created_at"],
    })
    report = build_report()
    print("\n[OK] Отдельный локальный кандидат создан и отвечает.")
    print(f"Модель: {tag}")
    print(f"Размер в Ollama: {format_bytes(ollama_size)}")
    print(f"Проверенных примеров: {data['examples']}")
    if data["examples"] < PRODUCTION_MIN_EXAMPLES:
        print(f"[PILOT ONLY] Для production по правилам проекта требуется не менее {PRODUCTION_MIN_EXAMPLES} примеров.")
    print("Текущий production-тег и настройки Render не менялись.")
    print(f"Manifest: {manifest_path}")
    print_report(report)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("report", "review", "prepare"))
    parser.add_argument("--model", default=DEFAULT_MODEL)
    args = parser.parse_args()
    TRAINING.mkdir(parents=True, exist_ok=True)
    if args.command == "review":
        return review_benchmark()
    if args.command == "prepare":
        return prepare_candidate()
    report = build_report(args.model)
    print_report(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
