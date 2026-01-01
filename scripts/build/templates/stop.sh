#!/bin/bash
# Stop script for {{BINARY_NAME}}

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

SERVICE_NAME="{{BINARY_NAME}}"
BINARY_NAME="{{BINARY_NAME}}"
CONFIG_DIR="/etc/${BINARY_NAME}"
DOCKER_DIR="$CONFIG_DIR/docker"

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[✓]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

if [ "$EUID" -ne 0 ]; then
    log_error "This script must be run as root or with sudo"
    exit 1
fi

# Detect installation mode
if [ -d "$DOCKER_DIR" ]; then
    # Docker mode
    log_info "Stopping ${SERVICE_NAME} Docker containers..."

    cd "$DOCKER_DIR" || exit 1

    # Check which docker-compose command is available
    if docker compose version &>/dev/null; then
        docker compose down
    else
        docker-compose down
    fi

    log_success "Docker containers stopped successfully"
    echo ""
    log_info "To start services again: cd $DOCKER_DIR && docker-compose up -d"
else
    # Systemd mode
    log_info "Stopping ${SERVICE_NAME} service..."
    systemctl stop "$SERVICE_NAME"

    # Wait for service to stop
    sleep 2

    if systemctl is-active --quiet "$SERVICE_NAME"; then
        log_error "Service is still running"
        exit 1
    else
        log_success "Service stopped successfully"
    fi
fi

