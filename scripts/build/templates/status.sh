#!/bin/bash
# Status check script for {{BINARY_NAME}}

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
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

echo "======================================"
echo "Service Status: ${SERVICE_NAME}"
echo "======================================"
echo ""

# Detect installation mode
if [ -d "$DOCKER_DIR" ]; then
    # Docker mode
    log_info "Installation mode: Docker"
    echo ""

    # Check if docker-compose is available
    if ! docker compose version &>/dev/null && ! docker-compose --version &>/dev/null; then
        log_error "docker-compose not found"
        exit 1
    fi

    # Change to docker directory
    cd "$DOCKER_DIR" || exit 1

    echo "======================================"
    echo "Docker Containers Status"
    echo "======================================"
    echo ""

    # Run docker-compose ps
    if docker compose version &>/dev/null; then
        docker compose ps
    else
        docker-compose ps
    fi

    echo ""
    echo "======================================"
    echo "Recent Logs (last 30 lines)"
    echo "======================================"
    echo ""

    # Show logs from all containers
    if docker compose version &>/dev/null; then
        docker compose logs --tail=30
    else
        docker-compose logs --tail=30
    fi

    echo ""
    echo "======================================"
    echo "Health Check"
    echo "======================================"
    HEALTH_URL="${OPS_HEALTH_URL:-http://localhost:3000/health}"
    if command -v curl >/dev/null 2>&1; then
        if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
            log_success "Health endpoint: OK"
        else
            log_error "Health endpoint: FAILED"
        fi
    else
        log_warn "curl not found, skipping health check"
    fi

    echo ""
    echo "======================================"
    echo "Configuration"
    echo "======================================"
    if [ -f "$DOCKER_DIR/.env" ]; then
        log_info "Docker env file: $DOCKER_DIR/.env"
        log_info "Last modified: $(stat -c %y "$DOCKER_DIR/.env" 2>/dev/null || stat -f %Sm "$DOCKER_DIR/.env" 2>/dev/null)"

        # Show seed data status
        if grep -q "SEED=true" "$DOCKER_DIR/.env"; then
            log_info "Seed data: Enabled"
        else
            log_info "Seed data: Disabled"
        fi
    else
        log_warn "Docker env file: NOT FOUND"
    fi

    echo ""
    log_info "Useful commands:"
    log_info "  - View logs: cd $DOCKER_DIR && docker-compose logs -f"
    log_info "  - Restart services: cd $DOCKER_DIR && docker-compose restart"
    log_info "  - Stop services: cd $DOCKER_DIR && docker-compose down"
    log_info "  - Start services: cd $DOCKER_DIR && docker-compose up -d"

else
    # Systemd mode
    log_info "Installation mode: Systemd"
    echo ""

    echo "======================================"
    echo "Systemd Service Status"
    echo "======================================"
    echo ""

    # Systemd status
    if systemctl is-active --quiet "$SERVICE_NAME"; then
        log_success "Status: RUNNING"
    else
        log_error "Status: STOPPED"
    fi
    echo ""

    # Show systemd status
    systemctl status "$SERVICE_NAME" --no-pager -l || true

    echo ""
    echo "======================================"
    echo "Recent Logs (last 20 lines)"
    echo "======================================"
    journalctl -u "$SERVICE_NAME" -n 20 --no-pager || true

    echo ""
    echo "======================================"
    echo "Health Check"
    echo "======================================"
    HEALTH_URL="${OPS_HEALTH_URL:-http://localhost:3000/health}"
    if command -v curl >/dev/null 2>&1; then
        if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
            log_success "Health endpoint: OK"
        else
            log_error "Health endpoint: FAILED"
        fi
    else
        log_warn "curl not found, skipping health check"
    fi

    echo ""
    echo "======================================"
    echo "Configuration"
    echo "======================================"
    if [ -f "$CONFIG_DIR/env" ]; then
        log_info "Config file: $CONFIG_DIR/env"
        log_info "Last modified: $(stat -c %y "$CONFIG_DIR/env" 2>/dev/null || stat -f %Sm "$CONFIG_DIR/env" 2>/dev/null)"
    else
        log_warn "Config file: NOT FOUND"
    fi

    echo ""
    log_info "Useful commands:"
    log_info "  - Start service: systemctl start $SERVICE_NAME"
    log_info "  - Stop service: systemctl stop $SERVICE_NAME"
    log_info "  - Restart service: systemctl restart $SERVICE_NAME"
    log_info "  - Enable on boot: systemctl enable $SERVICE_NAME"
    log_info "  - View logs: journalctl -u $SERVICE_NAME -f"
fi

echo ""

