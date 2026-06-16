#!/usr/bin/env python3
"""QLoRA fine-tune of Qwen2.5-7B on the nutrition SFT dataset.

Designed for an RTX 2060 (6 GB VRAM) via Unsloth (4-bit QLoRA + gradient
checkpointing). Training on CPU is impractically slow — use the GPU box for
this step; inference of the exported GGUF then runs on the CPU server.

Setup (CUDA machine):
    pip install "unsloth[cu121] @ git+https://github.com/unslothai/unsloth.git" trl peft datasets pyyaml

Usage:
    python train_qlora.py --data nutrition_sft.jsonl --out ./cookest-nutrition-merged
"""

from __future__ import annotations

import argparse

import yaml
from datasets import load_dataset
from transformers import TrainingArguments
from trl import SFTTrainer
from unsloth import FastLanguageModel


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", default="nutrition_sft.jsonl")
    ap.add_argument("--out", default="./cookest-nutrition-merged")
    ap.add_argument("--config", default="config.yaml")
    args = ap.parse_args()

    with open(args.config) as f:
        cfg = yaml.safe_load(f)

    model, tokenizer = FastLanguageModel.from_pretrained(
        model_name=cfg["base_model"],
        max_seq_length=cfg["max_seq_length"],
        load_in_4bit=True,
    )
    model = FastLanguageModel.get_peft_model(
        model,
        r=cfg["lora_r"],
        lora_alpha=cfg["lora_alpha"],
        lora_dropout=0.0,
        bias="none",
        use_gradient_checkpointing="unsloth",
        target_modules=[
            "q_proj", "k_proj", "v_proj", "o_proj",
            "gate_proj", "up_proj", "down_proj",
        ],
    )

    def to_chatml(ex: dict) -> dict:
        text = (
            f"<|im_start|>user\n{ex['instruction']}<|im_end|>\n"
            f"<|im_start|>assistant\n{ex['output']}<|im_end|>"
        )
        return {"text": text}

    dataset = load_dataset("json", data_files=args.data, split="train").map(to_chatml)

    trainer = SFTTrainer(
        model=model,
        tokenizer=tokenizer,
        train_dataset=dataset,
        dataset_text_field="text",
        max_seq_length=cfg["max_seq_length"],
        args=TrainingArguments(
            per_device_train_batch_size=cfg["batch_size"],
            gradient_accumulation_steps=cfg["grad_accum"],
            warmup_steps=5,
            num_train_epochs=cfg["epochs"],
            learning_rate=cfg["learning_rate"],
            fp16=True,
            logging_steps=10,
            optim="paged_adamw_8bit",
            output_dir="outputs",
            seed=42,
        ),
    )
    trainer.train()

    # Merge the LoRA adapter into the base weights (16-bit) for GGUF conversion.
    model.save_pretrained_merged(args.out, tokenizer, save_method="merged_16bit")
    print(f"Saved merged model to {args.out} — convert with export_gguf.sh")


if __name__ == "__main__":
    main()
