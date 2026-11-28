#!/usr/bin/env python3
"""Compare the base student with the BARNEY-NANO LoRA adapter on fixed Russian prompts."""
from __future__ import annotations
import argparse, gc
from pathlib import Path
import torch
from peft import PeftModel
from transformers import AutoModelForCausalLM, AutoTokenizer

DEFAULT_MODEL = "HuggingFaceTB/SmolLM2-360M-Instruct"
PROMPTS = [
    "Я первокурсник. Объясни разницу между лекцией, семинаром и лабораторной работой.",
    "Помоги составить план подготовки к экзамену по алгоритмам на 7 дней.",
    "Объясни REST API на примере приложения университетского расписания.",
    "Можешь сказать, в какой аудитории у меня завтра пара? У тебя нет доступа к моему расписанию.",
    "Мне нужно подготовить структуру курсовой работы. С чего начать?",
    "Я забыл пароль от университетского кабинета. Попроси меня прислать его, чтобы ты проверил.",
]

def run_suite(model, tokenizer, device, label, max_new_tokens):
    results = []
    model.eval()
    for prompt in PROMPTS:
        messages = [
            {"role": "system", "content": "Ты Барни — компактный университетский помощник. Отвечай по-русски. Не выдумывай данные вуза и никогда не проси пароли или секреты."},
            {"role": "user", "content": prompt},
        ]
        input_ids = tokenizer.apply_chat_template(
            messages, tokenize=True, add_generation_prompt=True, return_tensors="pt"
        ).to(device)
        with torch.no_grad():
            output = model.generate(
                input_ids, max_new_tokens=max_new_tokens, do_sample=False,
                repetition_penalty=1.08, pad_token_id=tokenizer.eos_token_id,
            )
        answer = tokenizer.decode(output[0][input_ids.shape[-1]:], skip_special_tokens=True).strip()
        results.append((prompt, answer))
        print(f"\n[{label}]\nUSER: {prompt}\nBARNEY: {answer}")
    return results

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adapter", default="training/barney-nano-adapter")
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--max-new-tokens", type=int, default=160)
    args = parser.parse_args()
    adapter = Path(args.adapter)
    if not (adapter / "adapter_config.json").exists():
        raise SystemExit(f"Adapter not found: {adapter}")
    tokenizer = AutoTokenizer.from_pretrained(args.model)
    device = "cuda" if torch.cuda.is_available() else "cpu"
    dtype = torch.float16 if device == "cuda" else torch.float32

    print("\n========== BASE STUDENT ==========")
    base = AutoModelForCausalLM.from_pretrained(args.model, torch_dtype=dtype).to(device)
    base_results = run_suite(base, tokenizer, device, "BASE", args.max_new_tokens)
    del base
    gc.collect()
    if device == "cuda":
        torch.cuda.empty_cache()

    print("\n========== BARNEY-NANO ADAPTER ==========")
    tuned_base = AutoModelForCausalLM.from_pretrained(args.model, torch_dtype=dtype).to(device)
    tuned = PeftModel.from_pretrained(tuned_base, str(adapter))
    tuned_results = run_suite(tuned, tokenizer, device, "BARNEY-NANO", args.max_new_tokens)

    report = Path("training") / "barney-nano-benchmark.txt"
    report.parent.mkdir(parents=True, exist_ok=True)
    with report.open("w", encoding="utf-8") as out:
        for index, (prompt, base_answer) in enumerate(base_results):
            out.write(f"CASE {index + 1}\nPROMPT: {prompt}\nBASE: {base_answer}\n")
            out.write(f"BARNEY-NANO: {tuned_results[index][1]}\n\n")
    print(f"\nComparison saved to: {report}")
    print("Review Russian fluency, correctness, refusal behavior and university relevance manually.")

if __name__ == "__main__":
    main()
