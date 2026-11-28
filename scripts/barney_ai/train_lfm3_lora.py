#!/usr/bin/env python3
"""LoRA SFT trainer for LiquidAI/LFM3-1.7B using the official TRL workflow."""
from __future__ import annotations
import argparse
from datasets import load_dataset
import torch
from peft import LoraConfig
from transformers import AutoModelForCausalLM, AutoTokenizer
from trl import SFTConfig, SFTTrainer

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dataset", default="data/barney_training/generated_sft.jsonl")
    parser.add_argument("--model", default="LiquidAI/LFM3-1.7B")
    parser.add_argument("--output", default="training/barney-lfm3-lora")
    parser.add_argument("--epochs", type=float, default=2.0)
    parser.add_argument("--batch-size", type=int, default=1)
    parser.add_argument("--grad-accum", type=int, default=8)
    parser.add_argument("--max-length", type=int, default=512)
    parser.add_argument("--lora-rank", type=int, default=8)
    parser.add_argument("--merge", action="store_true", help="Also save a merged HF checkpoint for GGUF conversion")
    args = parser.parse_args()

    if not torch.cuda.is_available():
        raise SystemExit("LoRA training requires a CUDA GPU in this starter script. Use a GPU notebook/runtime; Ollama inference alone does not train weights.")
    raw = load_dataset("json", data_files=args.dataset, split="train")
    if len(raw) < 30:
        raise SystemExit(f"Only {len(raw)} examples. Add and review more examples before training.")
    def to_prompt_completion(row):
        messages = row["messages"]
        if not messages or messages[-1]["role"] != "assistant":
            raise ValueError("Every training row must end with an assistant answer")
        return {
            "prompt": messages[:-1],
            "completion": [messages[-1]],
            "chat_template_kwargs": {"enable_thinking": False},
        }
    dataset = raw.map(to_prompt_completion, remove_columns=raw.column_names)
    split = dataset.train_test_split(test_size=0.1, seed=42)
    dtype = torch.bfloat16 if torch.cuda.is_available() and torch.cuda.is_bf16_supported() else torch.float16
    tokenizer = AutoTokenizer.from_pretrained(args.model, trust_remote_code=True, padding_side="left")
    model = AutoModelForCausalLM.from_pretrained(
        args.model, dtype=dtype, device_map="auto", trust_remote_code=True
    )
    model.config.use_cache = False
    lora = LoraConfig(
        r=args.lora_rank, lora_alpha=args.lora_rank * 2, lora_dropout=0.05, task_type="CAUSAL_LM",
        target_modules=["q_proj", "k_proj", "v_proj", "o_proj", "in_proj", "out_proj", "w1", "w2", "w3"],
    )
    config = SFTConfig(
        output_dir=args.output,
        num_train_epochs=args.epochs,
        per_device_train_batch_size=args.batch_size,
        per_device_eval_batch_size=1,
        gradient_accumulation_steps=args.grad_accum,
        learning_rate=1e-4,
        lr_scheduler_type="cosine",
        warmup_ratio=0.05,
        max_length=args.max_length,
        logging_steps=5,
        eval_strategy="epoch",
        save_strategy="epoch",
        load_best_model_at_end=True,
        metric_for_best_model="eval_loss",
        greater_is_better=False,
        bf16=(dtype == torch.bfloat16),
        fp16=(dtype == torch.float16 and torch.cuda.is_available()),
        gradient_checkpointing=True,
        report_to="none",
    )
    trainer = SFTTrainer(
        model=model, args=config, train_dataset=split["train"],
        eval_dataset=split["test"], processing_class=tokenizer, peft_config=lora,
    )
    trainer.train()
    trainer.save_model(args.output)
    tokenizer.save_pretrained(args.output)
    print(f"LoRA adapter saved to {args.output}")
    if args.merge:
        print("Moving model to CPU before merging to keep the 6 GB laptop GPU free...")
        trainer.model.to("cpu")
        merged = trainer.model.merge_and_unload()
        merged_path = args.output + "-merged"
        merged.save_pretrained(merged_path, safe_serialization=True)
        tokenizer.save_pretrained(merged_path)
        print(f"Merged Hugging Face model saved to {merged_path}")
    print("Evaluate the adapter on held-out conversations before deploying it.")

if __name__ == "__main__":
    main()
