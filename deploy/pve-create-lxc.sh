#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# Cookest — Proxmox VE LXC Container Creator
#
# Run on the Proxmox HOST (not inside an existing container):
#   chmod +x deploy/pve-create-lxc.sh
#   bash deploy/pve-create-lxc.sh
#
# What it does:
#   1. Downloads Ubuntu 22.04 template if not already present
#   2. Prompts for container settings (CTID, RAM, cores, storage, IP)
#   3. Creates an unprivileged LXC with Docker-compatible config
#   4. Starts the container
#   5. Injects and optionally runs the Cookest installer (install-cookest.sh)
#
# Requirements:
#   - Proxmox VE 7.x or 8.x
#   - Run as root on the PVE host
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

# ── Colour helpers ─────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { echo -e "${CYAN}[INFO]${NC}  $*"; }
ok()    { echo -e "${GREEN}[ OK ]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
fatal() { echo -e "${RED}[FAIL]${NC}  $*"; exit 1; }

# ── Prereq checks ─────────────────────────────────────────────────────────────
[[ $EUID -ne 0 ]] && fatal "This script must be run as root on the Proxmox host."
command -v pct   &>/dev/null || fatal "'pct' not found — are you on a Proxmox host?"
command -v pveam &>/dev/null || fatal "'pveam' not found — are you on a Proxmox host?"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_SCRIPT="${SCRIPT_DIR}/install-cookest.sh"
[[ -f "$INSTALL_SCRIPT" ]] || fatal "install-cookest.sh not found at ${INSTALL_SCRIPT}"

echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║        Cookest — Proxmox VE LXC Container Setup             ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# ── Template ───────────────────────────────────────────────────────────────────
TEMPLATE_NAME="ubuntu-22.04-standard_22.04-1_amd64.tar.zst"
TEMPLATE_STORAGE="local"
TEMPLATE_PATH="/var/lib/vz/template/cache/${TEMPLATE_NAME}"

if [[ ! -f "$TEMPLATE_PATH" ]]; then
    info "Ubuntu 22.04 template not found. Downloading..."
    # Find the exact available name in case minor version differs
    AVAILABLE=$(pveam available | grep "ubuntu-22.04-standard" | awk '{print $2}' | head -1)
    if [[ -z "$AVAILABLE" ]]; then
        fatal "No Ubuntu 22.04 standard template found in pveam. Check your subscription."
    fi
    info "Downloading: ${AVAILABLE}"
    pveam download "${TEMPLATE_STORAGE}" "${AVAILABLE}"
    TEMPLATE_NAME="$AVAILABLE"
    TEMPLATE_PATH="/var/lib/vz/template/cache/${TEMPLATE_NAME}"
fi
ok "Template ready: ${TEMPLATE_NAME}"

# ── Interactive prompts ────────────────────────────────────────────────────────
prompt() {
    local var_name="$1" prompt_text="$2" default="$3"
    local value
    read -r -p "  ${prompt_text} [${default}]: " value
    echo "${value:-$default}"
}

echo ""
echo "── Container Settings ────────────────────────────────────────────"

# Next available CTID
NEXT_CTID=$(pvesh get /cluster/nextid 2>/dev/null || echo "200")
CTID=$(prompt "CTID" "Container ID" "$NEXT_CTID")
[[ "$CTID" =~ ^[0-9]+$ ]] || fatal "CTID must be a number"
pct status "$CTID" &>/dev/null && fatal "Container ${CTID} already exists"

HOSTNAME=$(prompt "HOSTNAME" "Container hostname" "cookest")

echo ""
echo "  Resource tiers:"
echo "    1) Minimal  — 2 cores, 4 GB RAM, 20 GB disk  (no AI)"
echo "    2) Standard — 4 cores, 8 GB RAM, 40 GB disk  (external Ollama)"
echo "    3) Full AI  — 8 cores, 32 GB RAM, 80 GB disk (Ollama inside LXC)"
echo "    4) Custom"
TIER=$(prompt "TIER" "Choose tier" "2")

case "$TIER" in
    1) CORES=2;  MEMORY=4096;  SWAP=1024; DISK=20 ;;
    2) CORES=4;  MEMORY=8192;  SWAP=2048; DISK=40 ;;
    3) CORES=8;  MEMORY=32768; SWAP=4096; DISK=80 ;;
    4)
        CORES=$(prompt "CORES"  "vCPU count"    "4")
        MEMORY=$(prompt "MEMORY" "RAM in MB"     "8192")
        SWAP=$(prompt "SWAP"   "Swap in MB"    "2048")
        DISK=$(prompt "DISK"   "Disk in GB"    "40")
        ;;
    *) fatal "Invalid tier selection" ;;
esac

echo ""
echo "  Available storage pools:"
pvesm status --enabled | awk 'NR>1 {print "    -", $1}' 2>/dev/null || echo "    (could not list, check pvesm status)"
STORAGE=$(prompt "STORAGE" "Storage pool" "local-lvm")

echo ""
echo "  Network:"
echo "    Bridges detected:"
ip link show type bridge 2>/dev/null | awk -F': ' '/^[0-9]/{print "    -", $2}' || echo "    (check with: ip link)"
BRIDGE=$(prompt "BRIDGE" "Network bridge" "vmbr0")

echo "  IP configuration:"
echo "    1) DHCP (recommended for most setups)"
echo "    2) Static IP"
IP_MODE=$(prompt "IP_MODE" "Choose" "1")
if [[ "$IP_MODE" == "2" ]]; then
    STATIC_IP=$(prompt "STATIC_IP" "IP with prefix, e.g. 192.168.1.50/24" "")
    GATEWAY=$(prompt "GATEWAY"   "Gateway IP, e.g. 192.168.1.1"        "")
    NET_CONFIG="ip=${STATIC_IP},gw=${GATEWAY}"
else
    NET_CONFIG="ip=dhcp"
fi

echo ""
ROOT_PASS=$(prompt "ROOT_PASS" "Root password for the container" "$(openssl rand -base64 12 | tr -dc 'A-Za-z0-9' | head -c16)")

# ── Summary ────────────────────────────────────────────────────────────────────
echo ""
echo "── Summary ───────────────────────────────────────────────────────"
echo "  CTID       : ${CTID}"
echo "  Hostname   : ${HOSTNAME}"
echo "  Resources  : ${CORES} vCPUs / $((MEMORY/1024)) GB RAM / $((SWAP/1024)) GB swap / ${DISK} GB disk"
echo "  Storage    : ${STORAGE}"
echo "  Network    : bridge=${BRIDGE}, ${NET_CONFIG}"
echo ""
read -r -p "  Proceed? (y/N): " CONFIRM
[[ "$CONFIRM" =~ ^[Yy]$ ]] || { info "Aborted."; exit 0; }

# ── Create container ───────────────────────────────────────────────────────────
info "Creating LXC container ${CTID}..."

pct create "$CTID" "${TEMPLATE_STORAGE}:vztmpl/${TEMPLATE_NAME}" \
    --hostname "$HOSTNAME" \
    --cores "$CORES" \
    --memory "$MEMORY" \
    --swap "$SWAP" \
    --rootfs "${STORAGE}:${DISK}" \
    --net0 "name=eth0,bridge=${BRIDGE},firewall=1,${NET_CONFIG}" \
    --unprivileged 1 \
    --features "nesting=1,keyctl=1" \
    --ostype ubuntu \
    --password "$ROOT_PASS" \
    --start 0

ok "Container created."

# ── Docker-compatible LXC config ──────────────────────────────────────────────
info "Applying Docker-compatible LXC config..."
LXC_CONF="/etc/pve/lxc/${CTID}.conf"

# Only add if not already present
grep -q "apparmor" "$LXC_CONF" || cat >> "$LXC_CONF" <<'EOF'
lxc.apparmor.profile: unconfined
lxc.cgroup2.devices.allow: a
lxc.cap.drop:
EOF

ok "LXC config updated."

# ── Start container ────────────────────────────────────────────────────────────
info "Starting container ${CTID}..."
pct start "$CTID"

# Wait for container to be ready
ATTEMPTS=0
until pct exec "$CTID" -- test -f /etc/os-release 2>/dev/null; do
    ATTEMPTS=$((ATTEMPTS + 1))
    [[ $ATTEMPTS -ge 30 ]] && fatal "Container did not become ready after 60s"
    sleep 2
done
ok "Container is up."

# ── Inject installer script ────────────────────────────────────────────────────
info "Injecting install-cookest.sh into the container..."
pct push "$CTID" "$INSTALL_SCRIPT" /tmp/install-cookest.sh
pct exec "$CTID" -- chmod +x /tmp/install-cookest.sh
ok "Installer ready at /tmp/install-cookest.sh inside the container."

# ── Get container IP ───────────────────────────────────────────────────────────
sleep 3
CT_IP=$(pct exec "$CTID" -- bash -c "ip -4 addr show eth0 | grep -oP '(?<=inet\s)\d+(\.\d+){3}'" 2>/dev/null || echo "<unknown>")

# ── Offer to run installer ────────────────────────────────────────────────────
echo ""
read -r -p "  Run Cookest installer inside the container now? (Y/n): " RUN_NOW
if [[ ! "$RUN_NOW" =~ ^[Nn]$ ]]; then
    info "Running installer inside container ${CTID}..."
    pct exec "$CTID" -- bash /tmp/install-cookest.sh
else
    echo ""
    info "To install later, run:"
    echo "    pct enter ${CTID}"
    echo "    bash /tmp/install-cookest.sh"
fi

# ── Final summary ──────────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════════════════════╗"
echo "║  Container ${CTID} (${HOSTNAME}) is ready                            "
echo "║                                                                  "
echo "║  IP address  : ${CT_IP}                                          "
echo "║  SSH access  : ssh root@${CT_IP}                                 "
echo "║  Enter shell : pct enter ${CTID}                                 "
echo "║                                                                  "
echo "║  After install:                                                  "
echo "║    App API   : http://${CT_IP}:8080/health                       "
echo "║    Admin     : http://${CT_IP}:3000                              "
echo "╚══════════════════════════════════════════════════════════════════╝"
echo ""
