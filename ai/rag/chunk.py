"""Token-aware-ish text chunking for RAG ingestion.

We avoid heavy tokenizer dependencies and approximate tokens with words
(~1.3 tokens/word for English). Chunks break on paragraph boundaries where
possible and carry a small overlap so context isn't lost at the seams.
"""

from __future__ import annotations

import re


def _normalize(text: str) -> str:
    # Collapse runs of whitespace but keep paragraph breaks.
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    text = re.sub(r"[ \t]+", " ", text)
    text = re.sub(r"\n{3,}", "\n\n", text)
    return text.strip()


def chunk_text(text: str, target_words: int = 450, overlap_words: int = 80) -> list[str]:
    """Split text into overlapping chunks of roughly ``target_words`` words."""
    text = _normalize(text)
    if not text:
        return []

    paragraphs = [p.strip() for p in text.split("\n\n") if p.strip()]
    chunks: list[str] = []
    current: list[str] = []
    current_len = 0

    for para in paragraphs:
        words = para.split()
        if current_len + len(words) > target_words and current:
            chunks.append(" ".join(current))
            # Carry an overlap window into the next chunk.
            overlap = current[-overlap_words:] if overlap_words else []
            current = list(overlap)
            current_len = len(current)
        current.extend(words)
        current_len += len(words)

    if current:
        chunks.append(" ".join(current))

    # Drop tiny trailing fragments that add noise.
    return [c for c in chunks if len(c.split()) >= 20]
