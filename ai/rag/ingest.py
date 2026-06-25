#!/usr/bin/env python3
"""Ingest open nutrition books (PDFs) into the Cookest RAG knowledge base.

Pipeline: PDF -> text (pypdf) -> chunk -> embed (Ollama nomic-embed-text)
-> upsert into the `knowledge_chunks` pgvector table on the App API database.

The App API's EmbeddingService then retrieves these chunks at query time to
ground AI recipe and nutrition advice (RAG). This is how you "train" the
assistant on nutrition references without fine-tuning — see ../README.md.

Usage:
    pip install -r requirements.txt
    export APP_DATABASE_URL=postgresql://postgres:postgres@localhost:5433/cookest_app
    export OLLAMA_URL=http://localhost:11434
    python ingest.py /path/to/book.pdf /path/to/books_dir --title "Author, Title"

Idempotent: re-running replaces chunks for the same source file.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

import requests
import psycopg
from pypdf import PdfReader

from chunk import chunk_text

EMBED_DIM = 768  # nomic-embed-text


def db_url() -> str:
    url = os.environ.get("APP_DATABASE_URL") or os.environ.get("DATABASE_URL")
    if not url:
        sys.exit("Set APP_DATABASE_URL (or DATABASE_URL) to the App API database.")
    return url


def ollama_url() -> str:
    return os.environ.get("OLLAMA_URL", "http://localhost:11434").rstrip("/")


def embed_model() -> str:
    return os.environ.get("OLLAMA_EMBED_MODEL", "nomic-embed-text")


def embed(text: str) -> list[float]:
    resp = requests.post(
        f"{ollama_url()}/api/embeddings",
        json={"model": embed_model(), "prompt": text},
        timeout=120,
    )
    resp.raise_for_status()
    vec = resp.json().get("embedding", [])
    if len(vec) != EMBED_DIM:
        raise RuntimeError(
            f"Expected {EMBED_DIM}-dim embedding, got {len(vec)}. "
            f"Is '{embed_model()}' the right model?"
        )
    return vec


def vector_literal(vec: list[float]) -> str:
    return "[" + ",".join(repr(float(x)) for x in vec) + "]"


def extract_text(file_path: Path) -> str:
    if file_path.suffix.lower() == ".pdf":
        reader = PdfReader(str(file_path))
        return "\n\n".join((page.extract_text() or "") for page in reader.pages)
    else:
        return file_path.read_text(encoding="utf-8", errors="ignore")


def file_paths(inputs: list[str]) -> list[Path]:
    out: list[Path] = []
    for raw in inputs:
        p = Path(raw)
        if p.is_dir():
            out.extend(sorted(p.glob("*.pdf")))
            out.extend(sorted(p.glob("*.md")))
            out.extend(sorted(p.glob("*.txt")))
        elif p.suffix.lower() in (".pdf", ".md", ".txt") and p.exists():
            out.append(p)
        else:
            print(f"skip (unsupported file format): {raw}", file=sys.stderr)
    return out


def ingest_file(conn: psycopg.Connection, file_path: Path, title: str | None) -> int:
    source = file_path.name
    text = extract_text(file_path)
    chunks = chunk_text(text)
    if not chunks:
        print(f"  no extractable text in {source} — skipping")
        return 0

    with conn.cursor() as cur:
        # Replace any prior chunks for this source so re-ingest is clean.
        cur.execute("DELETE FROM knowledge_chunks WHERE source = %s", (source,))
        for i, content in enumerate(chunks):
            vec = embed(content)
            cur.execute(
                """
                INSERT INTO knowledge_chunks (source, title, chunk_index, content, embedding)
                VALUES (%s, %s, %s, %s, %s::vector)
                ON CONFLICT (source, chunk_index)
                DO UPDATE SET title = EXCLUDED.title,
                              content = EXCLUDED.content,
                              embedding = EXCLUDED.embedding
                """,
                (source, title, i, content, vector_literal(vec)),
            )
            if (i + 1) % 25 == 0:
                print(f"  {source}: embedded {i + 1}/{len(chunks)} chunks")
    conn.commit()
    return len(chunks)


def main() -> None:
    ap = argparse.ArgumentParser(description="Ingest nutrition documents (PDF, MD, TXT) into the RAG store.")
    ap.add_argument("inputs", nargs="+", help="Files and/or directories of documents")
    ap.add_argument("--title", default=None, help="Human-readable source title for citations")
    args = ap.parse_args()

    paths = file_paths(args.inputs)
    if not paths:
        sys.exit("No supported files found.")

    total = 0
    with psycopg.connect(db_url()) as conn:
        for p in paths:
            print(f"Ingesting {p} ...")
            total += ingest_file(conn, p, args.title)
    print(f"Done. {total} chunks across {len(paths)} file(s).")


if __name__ == "__main__":
    main()
