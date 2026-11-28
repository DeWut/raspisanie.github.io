#!/usr/bin/env python3
"""Merge the reviewed LoRA adapter into the student checkpoint for GGUF export."""
from __future__ import annotations
import argparse
from pathlib import Path
import torch
from peft import AutoPeftModelForCausalLM
from transformers import AutoTokenizer

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adapter", default="training/barney-nano-adapter")
    parser.add_argument("--output", default="training/barney-nano-merged")
    args = parser.parse_args()
    adapter = Path(args.adapter)
    if not (adapter / "adapter_config.json").exists(): raise SystemExit(f"Adapter not found: {adapter}")
    model = AutoPeftModelForCausalLM.from_pretrained(str(adapter), torch_dtype=torch.float32, device_map="cpu")
    merged = model.merge_and_unload()
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    merged.save_pretrained(output, safe_serialization=True)
    tokenizer = AutoTokenizer.from_pretrained(model.config._name_or_path)
    tokenizer.save_pretrained(output)
    print(f"Merged checkpoint saved: {output}")
    print("Next: convert with llama.cpp convert_hf_to_gguf.py and quantize to Q4_K_M.")
if __name__ == "__main__": main()
