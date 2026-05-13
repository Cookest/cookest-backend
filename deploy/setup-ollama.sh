#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# Cookest — Ollama CPU setup (Debian, AMD EPYC Zen4, tuned for 48-thread EPYC)
#
# Run as root on the dedicated Ollama server.
#
# Usage:
#   chmod +x deploy/setup-ollama.sh
#   sudo ./deploy/setup-ollama.sh
#   (re-run after a RAM upgrade — it will auto-select the better model)
#
# Model selection by available RAM:
#   8  GB  →  qwen2.5vl:3b             (~2.3 GB, ~8-15s/scan)
#   16 GB  →  qwen2.5vl:7b             (~5.5 GB, ~15-25s/scan)
#   32 GB  →  qwen2.5vl:7b + llama3.1 (both resident, no reload)
#
# EPYC Zen4 notes:
#   - AVX-512 VNNI is auto-detected by llama.cpp — no extra config needed
#   - Use physical core count for OLLAMA_NUM_THREADS, not logical (no hyperthreads)
#     Reason: llama.cpp matrix ops saturate cache per core; HT adds contention
#   - 48 threads = 24 physical cores on a typical Zen4 EPYC (e.g. EPYC 9354)
#     If you have a 48-core EPYC (96 threads), set PHYSICAL_CORES=48
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

# ── Auto-detect physical cores (works on any EPYC/Intel, single or dual socket) ──
# Counts unique (core_id, socket_id) pairs — ignores SMT/HT threads
PHYSICAL_CORES="${PHYSICAL_CORES:-$(lscpu -p=CORE,SOCKET | grep -v '^#' | sort -u | wc -l)}"
LOGICAL_THREADS=$(nproc --all)
echo "==> CPU: ${PHYSICAL_CORES} physical cores / ${LOGICAL_THREADS} logical threads"
echo "    Using ${PHYSICAL_CORES} threads for Ollama (physical cores only — HT excluded)"
OLLAMA_HOST="${OLLAMA_HOST:-0.0.0.0:11434}"
CHAT_MODEL="${OLLAMA_MODEL:-llama3.1}"
KEEP_ALIVE="${OLLAMA_KEEP_ALIVE:-10m}"

# ── Detect available RAM and pick the best fitting vision model ───────────────
TOTAL_RAM_MB=$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)
echo "==> Detected ${TOTAL_RAM_MB} MB RAM"
VISION_MODEL=""
VISION_MODEL_FALLBACK="${OLLAMA_VISION_MODEL_FALLBACK:-qwen2.5vl}"
VISION_MODEL_CANDIDATES=()

if [ "${TOTAL_RAM_MB}" -ge 28000 ]; then
    # ≥ 28 GB: run vision + chat resident simultaneously
    VISION_MODEL="${OLLAMA_VISION_MODEL:-qwen2.5vl:7b}"
    VISION_MODEL_CANDIDATES=("${VISION_MODEL}")
    PULL_CHAT=true
    echo "    Mode: FULL  — vision + chat both resident in RAM"
elif [ "${TOTAL_RAM_MB}" -ge 12000 ]; then
    # ≥ 12 GB: 7B vision, chat loads on demand
    VISION_MODEL="${OLLAMA_VISION_MODEL:-qwen2.5vl:7b}"
    VISION_MODEL_CANDIDATES=("${VISION_MODEL}")
    PULL_CHAT=false
    echo "    Mode: VISION-ONLY resident (chat loads on demand)"
else
    # < 12 GB (current 8 GB): try 3B first, then fall back to the base family tag
    VISION_MODEL="${OLLAMA_VISION_MODEL:-qwen2.5vl:3b}"
    VISION_MODEL_CANDIDATES=("${VISION_MODEL}" "${VISION_MODEL_FALLBACK}")
    PULL_CHAT=false
    echo "    Mode: CONSTRAINED (8 GB) — using 3B model (~2.3 GB)"
    echo "    Upgrade to ≥16 GB to unlock 7B for better accuracy"
fi

# ── Install Ollama (skip if already installed) ────────────────────────────────
if command -v ollama &>/dev/null; then
    echo "==> Ollama already installed ($(ollama --version 2>/dev/null || echo 'unknown version')), skipping install"
else
    echo "==> Installing Ollama..."
    curl -fsSL https://ollama.com/install.sh | sh
fi

# ── Tune systemd service for EPYC Zen4 ───────────────────────────────────────
echo "==> Writing EPYC-tuned systemd override..."
mkdir -p /etc/systemd/system/ollama.service.d
# Use printf to avoid heredoc variable-expansion surprises under set -u
printf '[Service]\n' > /etc/systemd/system/ollama.service.d/override.conf
# Expose on all interfaces so app-api can reach this server over the network
printf 'Environment="OLLAMA_HOST=%s"\n' "${OLLAMA_HOST}"              >> /etc/systemd/system/ollama.service.d/override.conf
# Physical cores only — HT threads hurt llama.cpp matrix-multiply cache perf
printf 'Environment="OLLAMA_NUM_THREADS=%s"\n' "${PHYSICAL_CORES}"   >> /etc/systemd/system/ollama.service.d/override.conf
# CPU-only — no GPU layers
printf 'Environment="OLLAMA_NUM_GPU=0"\n'                             >> /etc/systemd/system/ollama.service.d/override.conf
# Flash attention: -30% KV-cache RAM, faster long contexts
printf 'Environment="OLLAMA_FLASH_ATTENTION=1"\n'                     >> /etc/systemd/system/ollama.service.d/override.conf
# Keep model resident between requests (avoids ~10s cold-start reload)
printf 'Environment="OLLAMA_KEEP_ALIVE=%s"\n' "${KEEP_ALIVE}"        >> /etc/systemd/system/ollama.service.d/override.conf

systemctl daemon-reload
systemctl enable ollama
systemctl restart ollama

# ── Wait for service ──────────────────────────────────────────────────────────
echo "==> Waiting for Ollama to come up..."
ATTEMPTS=0
until curl -sf http://localhost:11434/api/tags > /dev/null 2>&1; do
    ATTEMPTS=$((ATTEMPTS + 1))
    if [ "${ATTEMPTS}" -ge 30 ]; then
        echo "ERROR: Ollama did not start after 60s"
        journalctl -u ollama --no-pager -n 20
        exit 1
    fi
    echo "    [${ATTEMPTS}/30] waiting..."
    sleep 2
done
echo "    Ready."

# ── Pull models ───────────────────────────────────────────────────────────────
echo ""
echo "==> Pulling vision model: ${VISION_MODEL}"
if [ "${VISION_MODEL}" = "qwen2.5vl:3b" ]; then
    echo "    (~2.3 GB download)"
else
    echo "    (~4.7 GB download)"
fi

VISION_PULL_OK=false
for MODEL in "${VISION_MODEL_CANDIDATES[@]}"; do
    if [ -z "${MODEL}" ]; then
        continue
    fi
    echo "    Trying ${MODEL}..."
    if ollama pull "${MODEL}"; then
        VISION_MODEL="${MODEL}"
        VISION_PULL_OK=true
        break
    fi
    echo "    WARN: failed to pull ${MODEL}"
done

if [ "${VISION_PULL_OK}" != true ]; then
    echo "ERROR: unable to pull any Ollama vision model"
    exit 1
fi

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
    echo "   sudo OLLAMA_VISION_MODEL=qwen2.5vl:7b ./setup-ollama.sh"
fi
