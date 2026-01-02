#!/bin/bash
# Restart script for {{BINARY_NAME}}

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

SERVICE_NAME="{{BINARY_NAME}}"
BINARY_NAME="{{BINARY_NAME}}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGE_DIR="$(dirname "$SCRIPT_DIR")"
DOCKER_DIR="$PACKAGE_DIR/docker"
MARKER_FILE="$PACKAGE_DIR/.docker-mode"

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[✓]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

if [ "$EUID" -ne 0 ]; then
    log_error "This script must be run as root or with sudo"
    exit 1
fi

# Detect installation mode
INSTALL_MODE="native"
if [ -f "$MARKER_FILE" ]; then
    INSTALL_MODE="docker"
fi

if [ "$INSTALL_MODE" = "docker" ]; then
    # Docker mode
    log_info "Restarting ${SERVICE_NAME} Docker containers..."

    if [ ! -d "$DOCKER_DIR" ]; then
        log_error "Docker directory not found: $DOCKER_DIR"
        log_error "Please run install.sh first"
        exit 1
    fi

    cd "$DOCKER_DIR" || exit 1

    # Check which docker-compose command is available
    if docker compose version &>/dev/null 2>&1; then
        docker compose restart
    else
        docker-compose restart
    fi

    log_success "Docker containers restarted successfully"
    echo ""
    log_info "View logs: cd $DOCKER_DIR && docker-compose logs -f"
else
    # Systemd mode
    log_info "Restarting ${SERVICE_NAME} service..."
    systemctl restart "$SERVICE_NAME"

    # Wait for service to be active
    sleep 2

    if systemctl is-active --quiet "$SERVICE_NAME"; then
        log_success "Service restarted successfully"
        systemctl status "$SERVICE_NAME" --no-pager
    else
        log_error "Failed to restart service"
        echo ""
        log_info "Check logs with: journalctl -u ${SERVICE_NAME} -n 50"
        exit 1
    fi
fi
