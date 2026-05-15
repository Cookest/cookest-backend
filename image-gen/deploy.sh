#!/usr/bin/env bash
# deploy-image-gen.sh
# Run this on the Debian server to install and start the image-gen service.
# Usage: bash deploy-image-gen.sh
set -euo pipefail

IMAGE_GEN_DIR="/opt/cookest/image-gen"
SERVICE_USER="cookest"
VENV_DIR="$IMAGE_GEN_DIR/.venv"

echo "=== Cookest Image Generation Service — Deployment ==="

# ── Install system dependencies ───────────────────────────────────────────────
echo "[1/6] Installing system dependencies..."
sudo apt-get update -qq
sudo apt-get install -y --no-install-recommends \
    python3.11 python3.11-venv python3-pip \
    libgl1 libglib2.0-0 \
    curl git

# ── Copy service files ────────────────────────────────────────────────────────
echo "[2/6] Copying service files..."
sudo mkdir -p "$IMAGE_GEN_DIR"
sudo cp -r . "$IMAGE_GEN_DIR/"
sudo chown -R "$SERVICE_USER:$SERVICE_USER" "$IMAGE_GEN_DIR"

# ── Create virtual environment ────────────────────────────────────────────────
echo "[3/6] Creating Python venv..."
sudo -u "$SERVICE_USER" python3.11 -m venv "$VENV_DIR"
sudo -u "$SERVICE_USER" "$VENV_DIR/bin/pip" install --upgrade pip
sudo -u "$SERVICE_USER" "$VENV_DIR/bin/pip" install -r "$IMAGE_GEN_DIR/requirements.txt"

# ── Create .env file ──────────────────────────────────────────────────────────
echo "[4/6] Writing .env..."
sudo tee "$IMAGE_GEN_DIR/.env" > /dev/null <<'ENV'
HOST=127.0.0.1
PORT=8082
SD_MODEL_ID=runwayml/stable-diffusion-v1-5
MODEL_CACHE_DIR=/opt/cookest/image-gen/model_cache
GENERATED_DIR=/opt/cookest/image-gen/generated
IMG_WIDTH=512
IMG_HEIGHT=512
INFERENCE_STEPS=25
GUIDANCE_SCALE=7.5
TORCH_DEVICE=cpu
NUM_WORKERS=1
MAX_QUEUE_SIZE=50
PUBLIC_BASE_URL=http://localhost:8082
IMAGE_GEN_TOKEN=change-me-in-production
ENV
sudo chown "$SERVICE_USER:$SERVICE_USER" "$IMAGE_GEN_DIR/.env"

# ── Systemd service ───────────────────────────────────────────────────────────
echo "[5/6] Installing systemd service..."
sudo tee /etc/systemd/system/cookest-image-gen.service > /dev/null <<SERVICE
[Unit]
Description=Cookest Image Generation Service
After=network.target

[Service]
Type=simple
User=$SERVICE_USER
WorkingDirectory=$IMAGE_GEN_DIR
EnvironmentFile=$IMAGE_GEN_DIR/.env
ExecStart=$VENV_DIR/bin/python run.py
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=cookest-image-gen

[Install]
WantedBy=multi-user.target
SERVICE

sudo systemctl daemon-reload
sudo systemctl enable cookest-image-gen
sudo systemctl restart cookest-image-gen

# ── Health check ──────────────────────────────────────────────────────────────
echo "[6/6] Waiting for service to start..."
sleep 5
if curl -sf http://localhost:8082/health; then
    echo -e "\n✅ Image generation service is running."
else
    echo -e "\n⚠️  Service may still be loading the model. Check with:"
    echo "   sudo journalctl -u cookest-image-gen -f"
fi
