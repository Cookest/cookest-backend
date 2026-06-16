# Cookest AI toolkit

Local, self-hosted AI for Cookest. Everything runs on the Ollama box
(CPU-only, 32 GB RAM, Zen4) reached via `OLLAMA_URL`.

There are two ways the assistant "learns" nutrition:

1. **RAG (primary, always on)** — open nutrition books are embedded and
   retrieved at query time, then injected into the chat / recipe-generation
   prompt and cited. Accurate, auditable, and updatable by just re-ingesting.
   This is the recommended path. See [`rag/`](rag/).
2. **Fine-tuning (optional, secondary)** — a QLoRA adapter that shapes the
   model's *tone and domain fluency*. It does **not** reliably memorize facts
   (that's RAG's job) and is capped at ~7B by the training GPU. See
   [`finetune/`](finetune/).

## Models (CPU box, 32 GB RAM)

| Role | Model | Env var | Approx RAM |
|------|-------|---------|-----------|
| Chat / recipe-gen | `qwen2.5:14b-instruct-q4_K_M` | `OLLAMA_MODEL` | ~9 GB |
| PDF-flyer OCR (vision) | `qwen2.5vl:7b` | `OLLAMA_VISION_MODEL` | ~6 GB |
| Embeddings (RAG) | `nomic-embed-text` (768-dim) | `OLLAMA_EMBED_MODEL` | ~0.3 GB |

All three stay resident (~15 GB) with headroom. Expect ~5–9 tok/s for the 14B
on this CPU — stream responses and keep `RAG_TOP_K` small (default 5).

```bash
ollama pull qwen2.5:14b-instruct-q4_K_M
ollama pull qwen2.5vl:7b
ollama pull nomic-embed-text
# Optional tuned chat model with CPU-friendly params:
ollama create cookest-chat -f models/Modelfile.chat   # then OLLAMA_MODEL=cookest-chat
```

The App API DB must have the `pgvector` extension (the compose file uses
`pgvector/pgvector:pg16`).

## Ingest nutrition books (RAG)

```bash
cd rag
pip install -r requirements.txt
export APP_DATABASE_URL=postgresql://postgres:postgres@localhost:5433/cookest_app
export OLLAMA_URL=http://localhost:11434
python ingest.py /path/to/nutrition_book.pdf --title "Author, Title"
# or a whole folder:
python ingest.py /path/to/books/
```

Re-running for the same file replaces its chunks (idempotent). Use
open-licensed / public-domain books you are allowed to redistribute.

Verify:

```sql
SELECT source, count(*) FROM knowledge_chunks GROUP BY source;
```

Then ask the in-app assistant a nutrition question — the App API's
`EmbeddingService` retrieves the top `RAG_TOP_K` chunks and the reply should
cite the source.
