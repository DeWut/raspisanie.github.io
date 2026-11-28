#!/usr/bin/env python3
"""Human review of LFM3-generated examples. Only approved rows enter training."""
from __future__ import annotations
import argparse, json, re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_SOURCE = ROOT / "data" / "barney_nano" / "teacher_raw.jsonl"
DEFAULT_OUTPUT = ROOT / "data" / "barney_nano" / "approved.jsonl"
SENSITIVE = re.compile(r"(?i)(\b[\w.+-]+@[\w.-]+\.[a-z]{2,}\b|\+?\d[\d\s()\-]{8,}\d|telegram\s*id|api[_ -]?key|password|парол[ья]|токен)")

def read_rows(path):
    with path.open("r", encoding="utf-8") as stream:
        for line_no, line in enumerate(stream, 1):
            if not line.strip():
                continue
            try:
                row = json.loads(line)
                messages = row["messages"]
                if len(messages) < 2 or messages[0]["role"] != "user" or messages[-1]["role"] != "assistant":
                    raise ValueError("invalid messages structure")
                yield line_no, row
            except Exception as exc:
                print(f"[SKIP] line {line_no}: {exc}")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()
    if not args.source.exists():
        raise SystemExit(f"Source not found: {args.source}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    approved = rejected = 0
    with args.output.open("a", encoding="utf-8") as out:
        for line_no, row in read_rows(args.source):
            messages = row["messages"]
            print("\n" + "=" * 76)
            print(f"Candidate from line {line_no} | topic: {row.get('topic', 'n/a')}")
            for message in messages:
                print(f"\n[{message['role'].upper()}]\n{message['content']}")
            text = "\n".join(m["content"] for m in messages)
            if SENSITIVE.search(text):
                print("\n[WARNING] Possible personal data or secret detected. Remove it before approval.")
            choice = input("\n[Y] approve  [E] edit answer  [N] reject  [Q] quit: ").strip().lower()
            if choice == "q":
                break
            if choice == "n" or choice == "":
                rejected += 1
                continue
            if choice == "e":
                print("Enter corrected assistant answer (one line; leave empty to reject):")
                answer = input("> ").strip()
                if not answer:
                    rejected += 1
                    continue
                messages[-1]["content"] = answer
            elif choice != "y":
                print("Unknown choice; rejected.")
                rejected += 1
                continue
            if SENSITIVE.search("\n".join(m["content"] for m in messages)):
                print("[REJECTED] Possible personal data or secret remains.")
                rejected += 1
                continue
            clean = {"messages": [{"role": m["role"], "content": m["content"]} for m in messages]}
            out.write(json.dumps(clean, ensure_ascii=False) + "\n")
            out.flush()
            approved += 1
    print(f"\nReview complete: approved={approved}, rejected={rejected}")
    print(f"Training file: {args.output}")

if __name__ == "__main__":
    main()
