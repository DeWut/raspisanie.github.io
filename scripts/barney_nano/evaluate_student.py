#!/usr/bin/env python3
"""Smoke-test the student LoRA adapter on Russian university prompts."""
from __future__ import annotations
import argparse
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
def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adapter", default="training/barney-nano-adapter")
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--max-new-tokens", type=int, default=180)
    args = parser.parse_args()
    adapter = Path(args.adapter)
    if not (adapter / "adapter_config.json").exists(): raise SystemExit(f"Adapter not found: {adapter}")
    tokenizer = AutoTokenizer.from_pretrained(args.model)
    model = AutoModelForCausalLM.from_pretrained(args.model, torch_dtype=torch.float16)
    model = PeftModel.from_pretrained(model, str(adapter))
    model.eval()
    device = "cuda" if torch.cuda.is_available() else "cpu"
    model.to(device)
    for prompt in PROMPTS:
        messages = [{"role":"system","content":"Ты Барни — компактный университетский помощник. Отвечай по-русски. Не выдумывай данные вуза и никогда не проси пароли или секреты."},
                    {"role":"user","content":prompt}]
        input_ids = tokenizer.apply_chat_template(messages, tokenize=True, add_generation_prompt=True, return_tensors="pt").to(device)
        with torch.no_grad():
            output = model.generate(input_ids, max_new_tokens=args.max_new_tokens,
                do_sample=True, temperature=0.55, top_p=0.9, repetition_penalty=1.08,
                pad_token_id=tokenizer.eos_token_id)
        answer = tokenizer.decode(output[0][input_ids.shape[-1]:], skip_special_tokens=True)
        print("\n" + "=" * 72 + "\nUSER: " + prompt + "\nBARNEY-NANO: " + answer.strip())
if __name__ == "__main__": main()
