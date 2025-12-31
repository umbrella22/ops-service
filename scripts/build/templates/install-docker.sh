#!/bin/bash
# Quick Docker Installation script for {{BINARY_NAME}} v{{VERSION}}
# This is a streamlined installer that only supports Docker deployment

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[✓]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

# Configuration
BINARY_NAME="{{BINARY_NAME}}"
VERSION="{{VERSION}}"
CONFIG_DIR="/etc/${BINARY_NAME}"
SERVICE_NAME="${BINARY_NAME}"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    log_error "This script must be run as root"
    exit 1
fi

log_info "Installing ${BINARY_NAME} v${VERSION} (Docker mode)"
echo ""

# Check Docker
if ! command -v docker &> /dev/null; then
    log_error "Docker is not installed"
    log_error "Please install Docker first:"
    log_error "  curl -fsSL https://get.docker.com | sh"
    exit 1
fi

# Check Docker Compose
if ! command -v docker-compose &> /dev/null && ! docker compose version &> /dev/null; then
    log_error "Docker Compose is not installed"
    log_error "Please install Docker Compose first"
    exit 1
fi
log_success "Docker and Docker Compose are installed"

# Create directories
log_info "Setting up directories..."
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mkdir -p "$CONFIG_DIR/docker"

# Install docker-compose configuration
log_info "Installing Docker Compose configuration..."
if [ -f "$SCRIPT_DIR/../docker/docker-compose.yml" ]; then
    cp "$SCRIPT_DIR/../docker/docker-compose.yml" "$CONFIG_DIR/docker/"
    log_success "Docker Compose configuration installed"
else
    log_error "Docker Compose configuration not found in package"
    exit 1
fi

# Create .env file
if [ ! -f "$CONFIG_DIR/docker/.env" ]; then
    log_info "Creating environment configuration..."
    cat > "$CONFIG_DIR/docker/.env" <<'EOF'
# PostgreSQL Configuration
POSTGRES_DB=ops_system
POSTGRES_USER=ops_user
POSTGRES_PASSWORD=$(openssl rand -base64 32 | tr -d "=+/" | cut -c1-25)

# Application Configuration
LOG_LEVEL=info
ALLOWED_IPS=
EOF
    chmod 640 "$CONFIG_DIR/docker/.env"
    log_success "Environment configuration created"
fi

# Create management scripts
log_info "Creating management scripts..."

# Start script
cat > "$CONFIG_DIR/docker/start.sh" <<'EOF'
#!/bin/bash
cd "$(dirname "$0")"
docker-compose up -d
echo "Services started. Use 'docker-compose logs -f' to view logs."
EOF
chmod +x "$CONFIG_DIR/docker/start.sh"

# Stop script
cat > "$CONFIG_DIR/docker/stop.sh" <<'EOF'
#!/bin/bash
cd "$(dirname "$0")"
docker-compose down
echo "Services stopped."
EOF
chmod +x "$CONFIG_DIR/docker/stop.sh"

# Status script
cat > "$CONFIG_DIR/docker/status.sh" <<'EOF'
#!/bin/bash
cd "$(dirname "$0")"
docker-compose ps
EOF
chmod +x "$CONFIG_DIR/docker/status.sh"

# Logs script
cat > "$CONFIG_DIR/docker/logs.sh" <<'EOF'
#!/bin/bash
cd "$(dirname "$0")"
docker-compose logs -f
EOF
chmod +x "$CONFIG_DIR/docker/logs.sh"

log_success "Management scripts created"

echo ""
log_success "========================================="
log_success "Installation completed successfully!"
log_success "========================================="
echo ""
echo "Configuration directory: $CONFIG_DIR/docker"
echo ""
echo "Quick start commands:"
echo "  cd $CONFIG_DIR/docker"
echo "  ./start.sh      # Start all services"
echo "  ./stop.sh       # Stop all services"
echo "  ./status.sh     # Check service status"
echo "  ./logs.sh       # View logs"
echo ""
echo "Services deployed:"
echo "  - PostgreSQL database (port 5432, localhost only)"
echo "  - API service (internal, accessible via Nginx)"
echo "  - Nginx reverse proxy (ports 80, 443)"
echo ""
echo "Access the application:"
echo "  HTTP:  http://localhost"
echo "  HTTPS: https://localhost"
echo ""
log_info "To start the services now, run:"
log_info "  cd $CONFIG_DIR/docker && ./start.sh"
echo ""
