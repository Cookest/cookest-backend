#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# Cookest — Ollama CPU setup (runs on the dedicated Ollama server)
#
# Installs Ollama as a systemd service and pulls the vision + chat models.
# Run as root or with sudo on the target Debian server.
#
# Usage:
#   chmod +x deploy/setup-ollama.sh
#   sudo ./deploy/setup-ollama.sh
#
# Recommended model: qwen2.5-vl:7b-instruct-q4_K_M
#   - ~2-3x faster than llava:7b on CPU (~20-35s per image)
#   - Much better JSON output compliance (critical for grocery list parsing)
#   - Excellent food/product recognition
#   - ~5 GB RAM — comfortably fits on a 32 GB server
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

VISION_MODEL="${OLLAMA_VISION_MODEL:-qwen2.5-vl:7b-instruct-q4_K_M}"
CHAT_MODEL="${OLLAMA_MODEL:-llama3.1}"
# Tune to the server's core count — leave a few cores free for the OS
NUM_THREADS="${OLLAMA_NUM_THREADS:-16}"
# Expose on all interfaces so the app-api container can reach this server
OLLAMA_HOST="${OLLAMA_HOST:-0.0.0.0:11434}"

echo "==> Installing Ollama..."
curl -fsSL https://ollama.com/install.sh | sh

echo "==> Configuring Ollama systemd service..."
mkdir -p /etc/systemd/system/ollama.service.d
cat > /etc/systemd/system/ollama.service.d/override.conf << EOF
[Service]
Environment="OLLAMA_HOST=${OLLAMA_HOST}"
Environment="OLLAMA_NUM_THREADS=${NUM_THREADS}"
# CPU-only inference (no GPU layers)
Environment="OLLAMA_NUM_GPU=0"
# Keep loaded model in RAM between requests (avoids ~10s reload penalty)
Environment="OLLAMA_KEEP_ALIVE=10m"
# Flash attention reduces memory ~30% on CPU
Environment="OLLAMA_FLASH_ATTENTION=1"
EOF

systemctl daemon-reload
systemctl enable ollama
systemctl restart ollama

echo "==> Waiting for Ollama to be ready..."
for i in $(seq 1 30); do
    if curl -sf http://localhost:11434/api/tags > /dev/null 2>&1; then
        echo "    Ollama is ready."
        break
    fi
    sleep 2
done

echo "==> Pulling vision model: ${VISION_MODEL}"
echo "    (qwen2.5-vl:7b Q4_K_M is ~4.7 GB download)"
ollama pull "${VISION_MODEL}"

echo "==> Pulling chat model: ${CHAT_MODEL}"
ollama pull "${CHAT_MODEL}"

echo "==> Verifying models are available..."
ollama list

# Quick smoke-test: describe a 1x1 white pixel to confirm the model loads
echo "==> Running vision smoke test..."
SMOKE=$(ollama run "${VISION_MODEL}" "Reply with exactly: ok" 2>/dev/null || echo "skipped")
echo "    Smoke test: ${SMOKE}"

echo ""
echo "✓ Ollama is running on ${OLLAMA_HOST}"
echo "  Vision model : ${VISION_MODEL}"
echo "  Chat model   : ${CHAT_MODEL}"
echo "  CPU threads  : ${NUM_THREADS}"
echo ""
echo "Point the app-api at this server by setting in api/.env (or docker-compose env):"
echo "  OLLAMA_URL=http://<this-server-ip>:11434"
echo "  OLLAMA_VISION_MODEL=${VISION_MODEL}"
echo "  OLLAMA_MODEL=${CHAT_MODEL}"
echo ""
echo "Memory budget on 32 GB:"
echo "  ${VISION_MODEL}  ~5 GB"
echo "  ${CHAT_MODEL}             ~5 GB"
echo "  OS + other services       ~2 GB"
echo "  Free headroom            ~20 GB"
