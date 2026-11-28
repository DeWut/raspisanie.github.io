#!/usr/bin/env python3
"""LoRA fine-tuning of a compact pretrained student using LFM3-distilled dialogues."""
from __future__ import annotations
import argparse, hashlib, json, shutil
from datetime import datetime, timezone
from pathlib import Path
import torch
from peft import LoraConfig, get_peft_model
from torch.utils.data import Dataset, random_split
from transformers import AutoModelForCausalLM, AutoTokenizer, Trainer, TrainingArguments

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DATA = ROOT / "data" / "barney_nano" / "approved.jsonl"
DEFAULT_OUTPUT = ROOT / "training" / "barney-nano-adapter"
DEFAULT_MODEL = "HuggingFaceTB/SmolLM2-360M-Instruct"

class DialogueDataset(Dataset):
    def __init__(self, path, tokenizer, max_length):
        self.rows = []
        with path.open("r", encoding="utf-8") as stream:
            for line in stream:
                if not line.strip():
                    continue
                messages = json.loads(line).get("messages", [])
                if len(messages) < 2 or messages[-1].get("role") != "assistant":
                    continue
                prompt = tokenizer.apply_chat_template(messages[:-1], tokenize=True, add_generation_prompt=True)
                full = tokenizer.apply_chat_template(messages, tokenize=True, add_generation_prompt=False)
                if isinstance(prompt, dict): prompt = prompt["input_ids"]
                if isinstance(full, dict): full = full["input_ids"]
                if hasattr(prompt, "tolist"): prompt = prompt.tolist()
                if hasattr(full, "tolist"): full = full.tolist()
                prompt_ids, full_ids = list(prompt), list(full)
                answer_ids = full_ids[len(prompt_ids):]
                if not answer_ids: continue
                if len(answer_ids) >= max_length:
                    answer_ids, prompt_ids = answer_ids[:max_length], []
                else:
                    prompt_ids = prompt_ids[-(max_length - len(answer_ids)):]
                ids = prompt_ids + answer_ids
                self.rows.append({"input_ids": ids, "labels": [-100] * len(prompt_ids) + answer_ids})
        if not self.rows:
            raise ValueError(f"No usable rows in {path}")
    def __len__(self): return len(self.rows)
    def __getitem__(self, index):
        row = self.rows[index]
        return {"input_ids": torch.tensor(row["input_ids"], dtype=torch.long),
                "labels": torch.tensor(row["labels"], dtype=torch.long)}

class PadCollator:
    def __init__(self, pad_id): self.pad_id = pad_id
    def __call__(self, features):
        size = max(len(x["input_ids"]) for x in features)
        ids, labels, masks = [], [], []
        for item in features:
            pad = size - len(item["input_ids"])
            ids.append(torch.cat([item["input_ids"], torch.full((pad,), self.pad_id, dtype=torch.long)]))
            labels.append(torch.cat([item["labels"], torch.full((pad,), -100, dtype=torch.long)]))
            masks.append(torch.cat([torch.ones(len(item["input_ids"]), dtype=torch.long), torch.zeros(pad, dtype=torch.long)]))
        return {"input_ids": torch.stack(ids), "labels": torch.stack(labels), "attention_mask": torch.stack(masks)}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data", type=Path, default=DEFAULT_DATA)
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--epochs", type=float, default=3)
    parser.add_argument("--batch-size", type=int, default=1)
    parser.add_argument("--grad-accum", type=int, default=16)
    parser.add_argument("--max-length", type=int, default=512)
    parser.add_argument("--rank", type=int, default=8)
    args = parser.parse_args()
    if not torch.cuda.is_available():
        raise SystemExit("CUDA GPU not detected. This training profile requires the RTX 4050.")
    if not args.data.exists(): raise SystemExit(f"Approved dataset not found: {args.data}")
    if args.max_length < 128 or args.max_length > 2048: raise SystemExit("--max-length must be 128..2048")
    tokenizer = AutoTokenizer.from_pretrained(args.model, use_fast=True)
    if tokenizer.pad_token_id is None: tokenizer.pad_token = tokenizer.eos_token
    tokenizer.padding_side = "right"
    dataset = DialogueDataset(args.data, tokenizer, args.max_length)
    dataset_hash = hashlib.sha256(args.data.read_bytes()).hexdigest()
    training_started = datetime.now(timezone.utc).isoformat()
    if len(dataset) < 30:
        raise SystemExit(f"Only {len(dataset)} examples. Add at least 30 for a smoke test; aim for 500+ diverse approved examples.")
    eval_size = max(1, int(len(dataset) * 0.1))
    train_dataset, eval_dataset = random_split(dataset, [len(dataset) - eval_size, eval_size],
        generator=torch.Generator().manual_seed(42))
    model = AutoModelForCausalLM.from_pretrained(args.model, torch_dtype=torch.float16)
    model.config.use_cache = False
    model.gradient_checkpointing_enable()
    model.enable_input_require_grads()
    model = get_peft_model(model, LoraConfig(
        r=args.rank, lora_alpha=args.rank * 2, lora_dropout=0.05, bias="none",
        task_type="CAUSAL_LM",
        target_modules=["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"]))
    model.print_trainable_parameters()
    trainable_params, total_params = model.get_nb_trainable_parameters()
    trainable_percent = (trainable_params / total_params * 100) if total_params else 0.0
    print(f"Trainable parameters: {trainable_params:,} / {total_params:,} ({trainable_percent:.5f}%)")
    args.output.mkdir(parents=True, exist_ok=True)
    trainer_dir = args.output / "_trainer"
    if trainer_dir.exists():
        shutil.rmtree(trainer_dir)
    training_args = TrainingArguments(
        output_dir=str(trainer_dir), num_train_epochs=args.epochs,
        per_device_train_batch_size=args.batch_size, per_device_eval_batch_size=1,
        gradient_accumulation_steps=args.grad_accum, learning_rate=1.5e-4,
        lr_scheduler_type="cosine", warmup_ratio=0.05, weight_decay=0.01,
        logging_steps=5, eval_strategy="epoch", save_strategy="epoch",
        save_total_limit=2, load_best_model_at_end=True,
        metric_for_best_model="eval_loss", greater_is_better=False,
        fp16=True, gradient_checkpointing=True,
        gradient_checkpointing_kwargs={"use_reentrant": False},
        optim="adamw_torch", report_to="none", remove_unused_columns=False)
    trainer = Trainer(model=model, args=training_args, train_dataset=train_dataset,
        eval_dataset=eval_dataset, data_collator=PadCollator(tokenizer.pad_token_id))
    trainer.train()
    trainer.save_model(str(args.output))
    trainer.state.save_to_json(str(args.output / "trainer_state.json"))
    tokenizer.save_pretrained(str(args.output))

    eval_losses = [
        float(row["eval_loss"])
        for row in trainer.state.log_history
        if "eval_loss" in row and isinstance(row["eval_loss"], (int, float))
    ]
    training_run = {
        "schema_version": 1,
        "started_at": training_started,
        "finished_at": datetime.now(timezone.utc).isoformat(),
        "base_model": args.model,
        "dataset_path": str(args.data.resolve()),
        "dataset_sha256": dataset_hash,
        "approved_examples": len(dataset),
        "trainable_parameters": int(trainable_params),
        "total_parameters_with_adapter": int(total_params),
        "trainable_percent": round(trainable_percent, 6),
        "epochs": args.epochs,
        "batch_size": args.batch_size,
        "gradient_accumulation": args.grad_accum,
        "max_length": args.max_length,
        "lora_rank": args.rank,
        "torch_version": torch.__version__,
        "cuda_version": torch.version.cuda,
        "gpu": torch.cuda.get_device_name(0),
        "best_eval_loss": trainer.state.best_metric,
        "min_logged_eval_loss": min(eval_losses) if eval_losses else None,
        "global_step": trainer.state.global_step,
    }
    (args.output / "training_run.json").write_text(
        json.dumps(training_run, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"Adapter saved: {args.output}")
    print(f"Trainable parameters: {trainable_params:,} / {total_params:,} ({trainable_percent:.5f}%)")
    print("Run evaluate_student.py, then inspect the readiness report and human review. Do not deploy based on training loss alone.")

    # A new successful training run invalidates reports and candidate pointers
    # from the previous weights. The old Ollama model tag is deliberately kept.
    for stale_name in (
        "barney-nano-benchmark.json",
        "barney-nano-benchmark.txt",
        "barney-nano-qa.json",
        "barney-nano-candidate.json",
        "barney-nano-readiness.json",
        "barney-nano-readiness.md",
    ):
        stale_path = ROOT / "training" / stale_name
        if stale_path.exists():
            stale_path.unlink()
    print("Previous benchmark/QA/release pointers invalidated for the new weights.")

if __name__ == "__main__": main()
