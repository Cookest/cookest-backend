#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# Cookest — Ollama CPU setup for Debian (32 GB RAM)
#
# Installs Ollama as a systemd service and pulls the vision + chat models.
# Run as root or with sudo on the target Debian server.
#
# Usage:
#   chmod +x deploy/setup-ollama.sh
#   sudo ./deploy/setup-ollama.sh
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

VISION_MODEL="${OLLAMA_VISION_MODEL:-llava:7b-v1.5-q4_K_M}"
CHAT_MODEL="${OLLAMA_MODEL:-llama3.1}"
# Number of CPU threads to use for inference (tune to your server's core count)
NUM_THREADS="${OLLAMA_NUM_THREADS:-16}"
# Bind Ollama to all interfaces so Docker containers can reach it
OLLAMA_HOST="${OLLAMA_HOST:-0.0.0.0:11434}"

echo "==> Installing Ollama..."
curl -fsSL https://ollama.com/install.sh | sh

echo "==> Configuring Ollama systemd service..."
# Override the default systemd unit to expose on all interfaces
# and cap CPU threads to leave headroom for Postgres + API
mkdir -p /etc/systemd/system/ollama.service.d
cat > /etc/systemd/system/ollama.service.d/override.conf << EOF
[Service]
Environment="OLLAMA_HOST=${OLLAMA_HOST}"
Environment="OLLAMA_NUM_THREADS=${NUM_THREADS}"
# Disable GPU layers entirely — CPU-only inference
Environment="OLLAMA_NUM_GPU=0"
# Keep loaded model in memory between requests (avoid reload penalty)
Environment="OLLAMA_KEEP_ALIVE=10m"
# Flash attention reduces memory usage ~30% on CPU
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
echo "    (This will download ~4.5 GB — grab a coffee)"
ollama pull "${VISION_MODEL}"

echo "==> Pulling chat model: ${CHAT_MODEL}"
ollama pull "${CHAT_MODEL}"

echo "==> Verifying models are available..."
ollama list

echo ""
echo "✓ Ollama is running on ${OLLAMA_HOST}"
echo "  Vision model : ${VISION_MODEL}"
echo "  Chat model   : ${CHAT_MODEL}"
echo "  CPU threads  : ${NUM_THREADS}"
echo ""
echo "To point the app-api at this Ollama instance, set in your .env:"
echo "  OLLAMA_URL=http://<server-ip>:11434"
echo "  OLLAMA_VISION_MODEL=${VISION_MODEL}"
echo "  OLLAMA_MODEL=${CHAT_MODEL}"
