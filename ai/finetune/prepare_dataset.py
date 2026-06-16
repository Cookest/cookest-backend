#!/usr/bin/env python3
"""Build a QLoRA instruction dataset from the ingested nutrition knowledge.

Reads chunks from `knowledge_chunks` (populated by ../rag/ingest.py) and uses the
local Ollama model to turn each passage into instruction/response pairs, written
as alpaca-style JSONL for train_qlora.py.

This reuses your RAG corpus as the fine-tuning source — run the RAG ingestion
first. Fine-tuning teaches *tone/domain fluency*; RAG remains the source of facts.

Usage:
    pip install -r ../rag/requirements.txt   # psycopg + requests
    export APP_DATABASE_URL=postgresql://postgres:postgres@localhost:5433/cookest_app
    export OLLAMA_URL=http://localhost:11434
    export OLLAMA_MODEL=qwen2.5:7b-instruct
    python prepare_dataset.py --out nutrition_sft.jsonl --max-chunks 500
"""

from __future__ import annotations

import argparse
import json
import os
import sys

import requests
import psycopg


def ollama_url() -> str:
    return os.environ.get("OLLAMA_URL", "http://localhost:11434").rstrip("/")


def gen_model() -> str:
    return os.environ.get("OLLAMA_MODEL", "qwen2.5:7b-instruct")


PROMPT = (
    "From the nutrition passage below, write 2 concise Q&A pairs that a cooking "
    "and nutrition assistant should know. Keep answers grounded in the passage.\n"
    'Return ONLY JSON: {{"pairs":[{{"instruction":"...","output":"..."}}]}}\n'
    'Passage:\n"""{passage}"""'
)


def gen_pairs(passage: str) -> list[dict]:
    resp = requests.post(
        f"{ollama_url()}/api/generate",
        json={
            "model": gen_model(),
            "prompt": PROMPT.format(passage=passage[:4000]),
            "stream": False,
            "format": "json",
        },
        timeout=180,
    )
    resp.raise_for_status()
    raw = resp.json().get("response", "{}")
    try:
        return json.loads(raw).get("pairs", [])
    except json.JSONDecodeError:
        return []


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="nutrition_sft.jsonl")
    ap.add_argument("--max-chunks", type=int, default=500)
    args = ap.parse_args()

    url = os.environ.get("APP_DATABASE_URL") or os.environ.get("DATABASE_URL")
    if not url:
        sys.exit("Set APP_DATABASE_URL (or DATABASE_URL).")

    written = 0
    with psycopg.connect(url) as conn, conn.cursor() as cur, open(args.out, "w") as f:
        cur.execute(
            "SELECT content FROM knowledge_chunks ORDER BY id LIMIT %s",
            (args.max_chunks,),
        )
        rows = cur.fetchall()
        if not rows:
            sys.exit("No knowledge_chunks found — run ../rag/ingest.py first.")
        for (content,) in rows:
            for pair in gen_pairs(content):
                instr = (pair.get("instruction") or "").strip()
                out = (pair.get("output") or "").strip()
                if instr and out:
                    f.write(json.dumps({"instruction": instr, "input": "", "output": out}) + "\n")
                    written += 1
            if written and written % 50 == 0:
                print(f"  wrote {written} pairs")
    print(f"Done. {written} instruction pairs -> {args.out}")


if __name__ == "__main__":
    main()
