#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# Cookest — Ollama CPU setup (Debian, AMD EPYC Zen4, tuned for 48-thread EPYC)
#
# Run as root on the dedicated Ollama server.
#
# Usage:
#   chmod +x deploy/setup-ollama.sh
#   sudo ./deploy/setup-ollama.sh
#
# Model selection by available RAM:
#   8  GB  →  qwen2.5-vl:3b            (~2.3 GB, ~8-15s/scan)
#   16 GB  →  qwen2.5-vl:7b-q4_K_M    (~5.5 GB, ~15-25s/scan)
#   32 GB  →  qwen2.5-vl:7b + llama3.1 (both resident, no reload)
#
# EPYC Zen4 notes:
#   - AVX-512 VNNI is auto-detected by llama.cpp — no extra config needed
#   - Use physical core count for OLLAMA_NUM_THREADS, not logical (no hyperthreads)
#     Reason: llama.cpp matrix ops saturate cache per core; HT adds contention
#   - 48 threads = 24 physical cores on a typical Zen4 EPYC (e.g. EPYC 9354)
#     If you have a 48-core EPYC (96 threads), set PHYSICAL_CORES=48
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

# ── Config (override via env) ─────────────────────────────────────────────────
PHYSICAL_CORES="${PHYSICAL_CORES:-24}"          # Zen4 physical cores (not HT threads)
OLLAMA_HOST="${OLLAMA_HOST:-0.0.0.0:11434}"
CHAT_MODEL="${OLLAMA_MODEL:-llama3.1}"
KEEP_ALIVE="${OLLAMA_KEEP_ALIVE:-10m}"

# ── Detect available RAM and pick the best fitting vision model ───────────────
TOTAL_RAM_MB=$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)
echo "==> Detected ${TOTAL_RAM_MB} MB RAM"

if [ "${TOTAL_RAM_MB}" -ge 28000 ]; then
    # ≥ 28 GB: run vision + chat resident simultaneously
    VISION_MODEL="${OLLAMA_VISION_MODEL:-qwen2.5-vl:7b-instruct-q4_K_M}"
    PULL_CHAT=true
    echo "    Mode: FULL  — vision + chat both resident in RAM"
elif [ "${TOTAL_RAM_MB}" -ge 12000 ]; then
    # ≥ 12 GB: 7B vision, chat loads on demand
    VISION_MODEL="${OLLAMA_VISION_MODEL:-qwen2.5-vl:7b-instruct-q4_K_M}"
    PULL_CHAT=false
    echo "    Mode: VISION-ONLY resident (chat loads on demand)"
else
    # < 12 GB (current 8 GB): 3B vision, fits with headroom
    VISION_MODEL="${OLLAMA_VISION_MODEL:-qwen2.5-vl:3b}"
    PULL_CHAT=false
    echo "    Mode: CONSTRAINED (8 GB) — using 3B model (~2.3 GB)"
    echo "    Upgrade to ≥16 GB to unlock 7B for better accuracy"
fi

# ── Install Ollama ────────────────────────────────────────────────────────────
echo "==> Installing Ollama..."
curl -fsSL https://ollama.com/install.sh | sh

# ── Tune systemd service for EPYC Zen4 ───────────────────────────────────────
echo "==> Writing EPYC-tuned systemd override..."
mkdir -p /etc/systemd/system/ollama.service.d
cat > /etc/systemd/system/ollama.service.d/override.conf << EOF
[Service]
# Expose on all interfaces so app-api can reach this server over the network
Environment="OLLAMA_HOST=${OLLAMA_HOST}"

# Use physical core count only — HT threads hurt llama.cpp matrix ops
# Zen4 AVX-512 VNNI saturates the L3 cache per core; extra threads add contention
Environment="OLLAMA_NUM_THREADS=${PHYSICAL_CORES}"

# CPU-only — no GPU layers
Environment="OLLAMA_NUM_GPU=0"

# Flash attention: reduces KV cache memory ~30%, speeds up longer contexts
Environment="OLLAMA_FLASH_ATTENTION=1"

# Keep loaded model resident for KEEP_ALIVE after last request
# Prevents cold-start reload penalty (~10s for 7B)
Environment="OLLAMA_KEEP_ALIVE=${KEEP_ALIVE}"

# llama.cpp will auto-detect AVX-512 VNNI on Zen4 — no extra flags needed
# MMAP the model file for faster cold start and lower RSS
Environment="OLLAMA_NOHISTORY=0"
EOF

systemctl daemon-reload
systemctl enable ollama
systemctl restart ollama

# ── Wait for service ──────────────────────────────────────────────────────────
echo "==> Waiting for Ollama to come up..."
for i in $(seq 1 30); do
    if curl -sf http://localhost:11434/api/tags > /dev/null 2>&1; then
        echo "    Ready."
        break
    fi
    echo "    [$i/30] waiting..."
    sleep 2
done

# ── Pull models ───────────────────────────────────────────────────────────────
echo ""
echo "==> Pulling vision model: ${VISION_MODEL}"
if [ "${VISION_MODEL}" = "qwen2.5-vl:3b" ]; then
    echo "    (~2.3 GB download)"
else
    echo "    (~4.7 GB download)"
fi
ollama pull "${VISION_MODEL}"

if [ "${PULL_CHAT}" = true ]; then
    echo "==> Pulling chat model: ${CHAT_MODEL} (~4.7 GB)"
    ollama pull "${CHAT_MODEL}"
fi

# ── Verify ────────────────────────────────────────────────────────────────────
echo ""
echo "==> Installed models:"
ollama list

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════════╗"
echo "║  Ollama ready on ${OLLAMA_HOST}"
echo "║"
echo "║  Vision model  : ${VISION_MODEL}"
echo "║  Chat model    : ${CHAT_MODEL} (pulled: ${PULL_CHAT})"
echo "║  CPU threads   : ${PHYSICAL_CORES} (physical cores)"
echo "║  RAM detected  : ${TOTAL_RAM_MB} MB"
echo "║"
echo "║  In app-api/.env (or docker-compose):  "
echo "║    OLLAMA_URL=http://$(hostname -I | awk '{print $1}'):11434"
echo "║    OLLAMA_VISION_MODEL=${VISION_MODEL}"
echo "╚══════════════════════════════════════════════════════╝"
echo ""
if [ "${TOTAL_RAM_MB}" -lt 12000 ]; then
    echo "⚠  RAM is under 12 GB. After upgrading:"
    echo "   sudo OLLAMA_VISION_MODEL=qwen2.5-vl:7b-instruct-q4_K_M ./setup-ollama.sh"
fi
