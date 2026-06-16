#!/usr/bin/env bash
# Convert a merged HF model to GGUF Q4_K_M and stage an Ollama Modelfile.
# Run on the training machine, then copy the .gguf + Modelfile to the CPU
# server and `ollama create` there.
#
# Prereqs: git clone https://github.com/ggerganov/llama.cpp && make -C llama.cpp
#
# Usage: ./export_gguf.sh ./cookest-nutrition-merged cookest-nutrition.gguf
set -euo pipefail

MERGED="${1:-./cookest-nutrition-merged}"
OUT="${2:-cookest-nutrition.gguf}"
LLAMA_CPP="${LLAMA_CPP:-$HOME/llama.cpp}"

if [[ ! -d "$LLAMA_CPP" ]]; then
  echo "Set LLAMA_CPP to your llama.cpp checkout (git clone ggerganov/llama.cpp)." >&2
  exit 1
fi

python "$LLAMA_CPP/convert_hf_to_gguf.py" "$MERGED" \
  --outfile "${OUT%.gguf}-f16.gguf" --outtype f16
"$LLAMA_CPP/llama-quantize" "${OUT%.gguf}-f16.gguf" "$OUT" Q4_K_M

cat > Modelfile.nutrition <<EOF
FROM ./$OUT
PARAMETER num_ctx 8192
PARAMETER num_thread 24
SYSTEM "You are Cookest AI, an evidence-based cooking and nutrition assistant."
EOF

echo "GGUF written to $OUT + Modelfile.nutrition"
echo "On the Ollama server:  ollama create cookest-nutrition -f Modelfile.nutrition"
echo "Then set OLLAMA_MODEL=cookest-nutrition (RAG still supplies the facts)."
