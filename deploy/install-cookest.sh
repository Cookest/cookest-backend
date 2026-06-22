#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# Cookest — Inside-LXC / Server Installer
#
# Run INSIDE the target server or LXC container (as root or with sudo):
#   chmod +x install-cookest.sh
#   bash install-cookest.sh
#
# What it does:
#   1. Installs Docker CE + Compose plugin
#   2. Creates /opt/cookest directory structure
#   3. Generates JWT secret
#   4. Prompts for config (domain/IP, AI, data source)
#   5. Writes .env and docker-compose.yml
#   6. Pulls Docker images and starts the stack
#   7. Optionally installs Nginx as a reverse proxy
#   8. Optionally installs Ollama for local AI
#
# Supported OS: Ubuntu 22.04, Ubuntu 24.04, Debian 12
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

# ── Colour helpers ─────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'
info()  { echo -e "${CYAN}[INFO]${NC}  $*"; }
ok()    { echo -e "${GREEN}[ OK ]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
fatal() { echo -e "${RED}[FAIL]${NC}  $*"; exit 1; }
step()  { echo -e "\n${BOLD}── $* ────────────────────────────────────────────────${NC}"; }

# ── Root check ─────────────────────────────────────────────────────────────────
[[ $EUID -ne 0 ]] && fatal "Run this script as root (or with sudo)."

# ── OS detection ───────────────────────────────────────────────────────────────
. /etc/os-release 2>/dev/null || fatal "Cannot read /etc/os-release"
case "${ID}" in
    ubuntu|debian) : ;;
    *) fatal "Unsupported OS: ${ID}. This script supports Ubuntu and Debian." ;;
esac
CODENAME="${VERSION_CODENAME:-}"
[[ -z "$CODENAME" ]] && fatal "Could not determine OS codename."

echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║            Cookest Self-Hosting Installer                    ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
info "Detected: ${PRETTY_NAME}"

# ── Helper ─────────────────────────────────────────────────────────────────────
prompt() {
    local var_name="$1" prompt_text="$2" default="$3"
    local value
    read -r -p "  ${prompt_text} [${default}]: " value
    echo "${value:-$default}"
}

prompt_yn() {
    local question="$1" default="${2:-y}"
    local answer
    if [[ "$default" == "y" ]]; then
        read -r -p "  ${question} (Y/n): " answer
        [[ ! "$answer" =~ ^[Nn]$ ]]
    else
        read -r -p "  ${question} (y/N): " answer
        [[ "$answer" =~ ^[Yy]$ ]]
    fi
}

INSTALL_DIR="/opt/cookest"

# ══════════════════════════════════════════════════════════════════════════════
step "Step 1: Install Docker"
# ══════════════════════════════════════════════════════════════════════════════

if command -v docker &>/dev/null; then
    DOCKER_VER=$(docker --version | grep -oP '\d+\.\d+' | head -1)
    ok "Docker already installed (${DOCKER_VER})"
else
    info "Installing Docker CE from the official repository..."

    apt-get update -qq
    apt-get install -y -qq ca-certificates curl gnupg lsb-release

    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL "https://download.docker.com/linux/${ID}/gpg" \
        | gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    chmod a+r /etc/apt/keyrings/docker.gpg

    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] \
        https://download.docker.com/linux/${ID} ${CODENAME} stable" \
        | tee /etc/apt/sources.list.d/docker.list > /dev/null

    apt-get update -qq
    apt-get install -y -qq docker-ce docker-ce-cli containerd.io docker-compose-plugin

    systemctl enable --now docker
    ok "Docker installed and started."
fi

# Verify Compose plugin
docker compose version &>/dev/null || fatal "Docker Compose plugin not found. Install docker-compose-plugin."

# ══════════════════════════════════════════════════════════════════════════════
step "Step 2: Configuration"
# ══════════════════════════════════════════════════════════════════════════════

echo ""
info "Enter your deployment settings:"
echo ""

# Detect primary IP for the default
PRIMARY_IP=$(ip -4 route get 1 2>/dev/null | awk '{print $7; exit}' || echo "localhost")

HOST_ADDR=$(prompt "HOST_ADDR" "Your server IP or domain (used by the mobile app)" "$PRIMARY_IP")
CORS_ORIGIN="http://${HOST_ADDR}:3000"
if [[ "$HOST_ADDR" != *"localhost"* && "$HOST_ADDR" != 192.* && "$HOST_ADDR" != 10.* && "$HOST_ADDR" != 172.* ]]; then
    CORS_ORIGIN="https://${HOST_ADDR}"
fi

echo ""
echo "  Data source options:"
echo "    local     — use only local PostgreSQL (recommended, no external APIs)"
echo "    hybrid    — local first, fall back to FatSecret if you supply keys"
echo "    fatsecret — FatSecret only (requires FS_CLIENT_ID and FS_CLIENT_SECRET)"
FOOD_DATA_SOURCE=$(prompt "FOOD_DATA_SOURCE" "Food data source" "local")

FS_CLIENT_ID=""
FS_CLIENT_SECRET=""
if [[ "$FOOD_DATA_SOURCE" == "hybrid" || "$FOOD_DATA_SOURCE" == "fatsecret" ]]; then
    FS_CLIENT_ID=$(prompt "FS_CLIENT_ID" "FatSecret Client ID" "")
    FS_CLIENT_SECRET=$(prompt "FS_CLIENT_SECRET" "FatSecret Client Secret" "")
fi

echo ""
INSTALL_OLLAMA=false
USE_BUNDLED_OLLAMA=false
if prompt_yn "Enable local AI features (recipe generation, receipt scanning)?" "y"; then
    echo ""
    echo "  Ollama options:"
    echo "    1) Run Ollama inside this container (downloads ~5-10 GB models)"
    echo "    2) Use an existing Ollama server on this host or LAN"
    echo "    3) Skip AI for now"
    OLLAMA_CHOICE=$(prompt "OLLAMA_CHOICE" "Choose" "1")

    case "$OLLAMA_CHOICE" in
        1)
            INSTALL_OLLAMA=true
            USE_BUNDLED_OLLAMA=true
            OLLAMA_URL="http://ollama:11434"
            ;;
        2)
            OLLAMA_URL=$(prompt "OLLAMA_URL" "Ollama URL" "http://192.168.1.x:11434")
            ;;
        *)
            OLLAMA_URL="http://host.docker.internal:11434"
            warn "AI features disabled. Set OLLAMA_URL in .env later to enable."
            ;;
    esac
else
    OLLAMA_URL="http://host.docker.internal:11434"
fi

# Detect available RAM for model selection
TOTAL_RAM_GB=$(awk '/MemTotal/{print int($2/1024/1024)}' /proc/meminfo)
if [[ "$TOTAL_RAM_GB" -ge 28 ]]; then
    OLLAMA_MODEL="llama3.1:8b"
    OLLAMA_VISION_MODEL="qwen2.5vl:7b"
elif [[ "$TOTAL_RAM_GB" -ge 12 ]]; then
    OLLAMA_MODEL="llama3.1:8b"
    OLLAMA_VISION_MODEL="qwen2.5vl:7b"
else
    OLLAMA_MODEL="llama3.2:3b"
    OLLAMA_VISION_MODEL="qwen2.5vl:3b"
    warn "Low RAM (${TOTAL_RAM_GB} GB) — using 3B models. Upgrade to 16+ GB for better accuracy."
fi

echo ""
INSTALL_NGINX=false
if prompt_yn "Install Nginx as reverse proxy?" "n"; then
    INSTALL_NGINX=true
fi

# ══════════════════════════════════════════════════════════════════════════════
step "Step 3: Create directory structure"
# ══════════════════════════════════════════════════════════════════════════════

mkdir -p "${INSTALL_DIR}"/{app-db,food-db,pdfs,imports,ollama,backups}
info "Created ${INSTALL_DIR}/"
ok "Directories created."

# ══════════════════════════════════════════════════════════════════════════════
step "Step 4: Generate secrets and write .env"
# ══════════════════════════════════════════════════════════════════════════════

JWT_SECRET=$(openssl rand -hex 32)
info "Generated JWT secret."

cat > "${INSTALL_DIR}/.env" <<EOF
# ─── SYSTEM ──────────────────────────────────────────────
SELF_HOSTED=true

# ─── SECURITY ────────────────────────────────────────────
JWT_SECRET=${JWT_SECRET}

# ─── DATA SOURCES ────────────────────────────────────────
FOOD_DATA_SOURCE=${FOOD_DATA_SOURCE}
FS_CLIENT_ID=${FS_CLIENT_ID}
FS_CLIENT_SECRET=${FS_CLIENT_SECRET}

# ─── NETWORKING ──────────────────────────────────────────
CORS_ORIGIN=${CORS_ORIGIN}

# ─── LOCAL AI ────────────────────────────────────────────
OLLAMA_URL=${OLLAMA_URL}
OLLAMA_MODEL=${OLLAMA_MODEL}
OLLAMA_VISION_MODEL=${OLLAMA_VISION_MODEL}
OLLAMA_VISION_TIMEOUT_SECS=120
OLLAMA_EMBED_MODEL=nomic-embed-text
EOF

chmod 600 "${INSTALL_DIR}/.env"
ok ".env written to ${INSTALL_DIR}/.env"

# ══════════════════════════════════════════════════════════════════════════════
step "Step 5: Write docker-compose.yml"
# ══════════════════════════════════════════════════════════════════════════════

# Build the Ollama service block conditionally
OLLAMA_SERVICE=""
if [[ "$USE_BUNDLED_OLLAMA" == "true" ]]; then
OLLAMA_SERVICE='
  ollama:
    image: ollama/ollama:latest
    container_name: cookest_ollama
    restart: unless-stopped
    volumes:
      - ./ollama:/root/.ollama
    ports:
      - "11434:11434"
'
fi

cat > "${INSTALL_DIR}/docker-compose.yml" <<COMPOSE
name: cookest

services:
  # ── Databases ────────────────────────────────────────────
  app-db:
    image: pgvector/pgvector:pg16
    container_name: cookest_app_db
    restart: unless-stopped
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
      POSTGRES_DB: cookest_app
    volumes:
      - ./app-db:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres -d cookest_app"]
      interval: 5s
      timeout: 5s
      retries: 5

  food-db:
    image: postgres:16-alpine
    container_name: cookest_food_db
    restart: unless-stopped
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
      POSTGRES_DB: cookest_food
    volumes:
      - ./food-db:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres -d cookest_food"]
      interval: 5s
      timeout: 5s
      retries: 5

  # ── Food API ──────────────────────────────────────────────
  food-api:
    image: ghcr.io/cookest/food-api:latest
    container_name: cookest_food_api
    restart: unless-stopped
    env_file: .env
    environment:
      FOOD_DATABASE_URL: postgresql://postgres:postgres@food-db:5432/cookest_food
      FOOD_HOST: 0.0.0.0
      FOOD_PORT: 8081
      FOOD_CORS_ORIGIN: "*"
    volumes:
      - ./imports:/data/imports:ro
    depends_on:
      food-db:
        condition: service_healthy

  # ── App API ───────────────────────────────────────────────
  app-api:
    image: ghcr.io/cookest/app-api:latest
    container_name: cookest_app_api
    restart: unless-stopped
    env_file: .env
    environment:
      APP_DATABASE_URL: postgresql://postgres:postgres@app-db:5432/cookest_app
      HOST: 0.0.0.0
      PORT: 8080
      FOOD_API_URL: http://food-api:8081
      PDF_UPLOAD_DIR: /data/pdfs
      SELF_HOSTED: "true"
    ports:
      - "8080:8080"
    volumes:
      - ./pdfs:/data/pdfs
      - ./imports:/data/imports:ro
    depends_on:
      app-db:
        condition: service_healthy
      food-api:
        condition: service_started

  # ── Admin Panel ───────────────────────────────────────────
  admin:
    image: ghcr.io/cookest/admin:latest
    container_name: cookest_admin
    restart: unless-stopped
    environment:
      NEXT_PUBLIC_APP_API_URL: http://${HOST_ADDR}:8080
      APP_API_INTERNAL_URL: http://app-api:8080
    ports:
      - "3000:3000"
    depends_on:
      - app-api
${OLLAMA_SERVICE}
COMPOSE

ok "docker-compose.yml written to ${INSTALL_DIR}/docker-compose.yml"

# ══════════════════════════════════════════════════════════════════════════════
step "Step 6: Start Cookest"
# ══════════════════════════════════════════════════════════════════════════════

cd "${INSTALL_DIR}"

info "Pulling Docker images (this may take a few minutes)..."
docker compose pull

info "Starting services..."
docker compose up -d

# Wait for app-api health check
info "Waiting for app-api to become healthy..."
ATTEMPTS=0
until curl -sf http://localhost:8080/health > /dev/null 2>&1; do
    ATTEMPTS=$((ATTEMPTS + 1))
    if [[ $ATTEMPTS -ge 60 ]]; then
        warn "app-api health check timed out after 120s. Check logs:"
        echo "    docker compose -f ${INSTALL_DIR}/docker-compose.yml logs app-api --tail=30"
        break
    fi
    printf "."
    sleep 2
done
echo ""
ok "Services are up."

# ══════════════════════════════════════════════════════════════════════════════
step "Step 7: Pull AI models"
# ══════════════════════════════════════════════════════════════════════════════

if [[ "$INSTALL_OLLAMA" == "true" ]]; then
    info "Waiting for Ollama container to start..."
    ATTEMPTS=0
    until docker compose exec -T ollama ollama list &>/dev/null; do
        ATTEMPTS=$((ATTEMPTS + 1))
        [[ $ATTEMPTS -ge 30 ]] && { warn "Ollama did not start — skip model pull"; INSTALL_OLLAMA="false"; break; }
        sleep 2
    done

    if [[ "$INSTALL_OLLAMA" == "true" ]]; then
        info "Pulling ${OLLAMA_VISION_MODEL} (vision/OCR model)..."
        docker compose exec -T ollama ollama pull "${OLLAMA_VISION_MODEL}"

        if [[ "$TOTAL_RAM_GB" -ge 28 ]]; then
            info "Pulling ${OLLAMA_MODEL} (chat model)..."
            docker compose exec -T ollama ollama pull "${OLLAMA_MODEL}"
        else
            warn "Skipping chat model pull (need 28+ GB RAM to run both resident). Pull manually later:"
            echo "    docker compose exec ollama ollama pull ${OLLAMA_MODEL}"
        fi
        ok "AI models ready."
    fi
fi

# ══════════════════════════════════════════════════════════════════════════════
step "Step 8: Nginx reverse proxy (optional)"
# ══════════════════════════════════════════════════════════════════════════════

if [[ "$INSTALL_NGINX" == "true" ]]; then
    info "Installing Nginx..."
    apt-get install -y -qq nginx

    NGINX_CONF="/etc/nginx/sites-available/cookest"
    cat > "$NGINX_CONF" <<NGINX
server {
    listen 80;
    server_name ${HOST_ADDR};

    # App API
    location /api/ {
        proxy_pass         http://127.0.0.1:8080;
        proxy_set_header   Host \$host;
        proxy_set_header   X-Real-IP \$remote_addr;
        proxy_set_header   X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header   X-Forwarded-Proto \$scheme;
        proxy_read_timeout 300s;
        client_max_body_size 50M;
    }

    location = /health {
        proxy_pass http://127.0.0.1:8080;
    }

    # Admin panel
    location / {
        proxy_pass       http://127.0.0.1:3000;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
    }
}
NGINX

    ln -sf "$NGINX_CONF" /etc/nginx/sites-enabled/cookest
    rm -f /etc/nginx/sites-enabled/default
    nginx -t && systemctl enable --now nginx && systemctl reload nginx
    ok "Nginx configured and running."

    # Offer certbot if this is a real domain
    if [[ "$HOST_ADDR" != *"."*"."*"."* ]] 2>/dev/null; then
        # Looks like a domain name, not an IP
        if prompt_yn "Obtain a Let's Encrypt TLS certificate for ${HOST_ADDR}?" "y"; then
            apt-get install -y -qq certbot python3-certbot-nginx
            certbot --nginx -d "${HOST_ADDR}" --non-interactive --agree-tos \
                -m "admin@${HOST_ADDR}" --redirect || warn "Certbot failed. Run manually: certbot --nginx -d ${HOST_ADDR}"
        fi
    fi
fi

# ══════════════════════════════════════════════════════════════════════════════
step "Step 9: Set up automatic backups (cron)"
# ══════════════════════════════════════════════════════════════════════════════

CRON_FILE="/etc/cron.d/cookest-backup"
cat > "$CRON_FILE" <<CRON
# Cookest daily database backups at 03:00
SHELL=/bin/bash
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
0 3 * * * root cd ${INSTALL_DIR} && docker compose exec -T app-db pg_dump -U postgres cookest_app | gzip > ${INSTALL_DIR}/backups/app_\$(date +\%F).sql.gz 2>/dev/null; docker compose exec -T food-db pg_dump -U postgres cookest_food | gzip > ${INSTALL_DIR}/backups/food_\$(date +\%F).sql.gz 2>/dev/null
# Prune backups older than 30 days
0 4 * * * root find ${INSTALL_DIR}/backups -name "*.sql.gz" -mtime +30 -delete 2>/dev/null
CRON
ok "Daily backup cron job written to ${CRON_FILE}"

# ══════════════════════════════════════════════════════════════════════════════
# Final summary
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "╔═══════════════════════════════════════════════════════════════════╗"
echo "║  ✓  Cookest is installed and running!                             ║"
echo "║                                                                   ║"
echo "║  Install dir  : ${INSTALL_DIR}                                    ║"
echo "║                                                                   ║"
echo "║  App API      : http://${HOST_ADDR}:8080/health                   ║"
echo "║  Admin panel  : http://${HOST_ADDR}:3000                          ║"
if [[ "$INSTALL_NGINX" == "true" ]]; then
echo "║  Via Nginx    : http://${HOST_ADDR}                               ║"
fi
echo "║                                                                   ║"
echo "║  Useful commands (from ${INSTALL_DIR}):                           ║"
echo "║    docker compose ps           — service status                   ║"
echo "║    docker compose logs -f      — live logs                        ║"
echo "║    docker compose pull         — pull latest images               ║"
echo "║    docker compose down && up -d — restart all                     ║"
echo "║                                                                   ║"
echo "║  Next steps:                                                      ║"
echo "║    1. Open the admin panel and create an admin account            ║"
echo "║    2. Set is_admin=true in the DB for your user                   ║"
echo "║    3. Import recipe data via Database → Dataset Import            ║"
echo "║    4. Connect the mobile app using the custom server panel        ║"
echo "╚═══════════════════════════════════════════════════════════════════╝"
echo ""
