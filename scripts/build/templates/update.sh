#!/bin/bash
# Update script for {{BINARY_NAME}}

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
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGE_DIR="$(dirname "$SCRIPT_DIR")"
DOCKER_DIR="$PACKAGE_DIR/docker"
MARKER_FILE="$PACKAGE_DIR/.docker-mode"
INSTALL_DIR="/usr/local/bin"
SERVICE_NAME="${BINARY_NAME}"
CONFIG_DIR="/etc/${BINARY_NAME}"
DATA_DIR="/var/lib/${BINARY_NAME}"
BACKUP_DIR="/var/backups/${BINARY_NAME}"

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
    if [ ! -d "$DOCKER_DIR" ]; then
        log_error "Docker directory not found: $DOCKER_DIR"
        log_error "Please run install.sh first"
        exit 1
    fi
fi

log_info "Updating ${BINARY_NAME} to v${VERSION} (${INSTALL_MODE} mode)"
echo ""

# Step 1: Backup current installation
log_info "Step 1: Backing up current installation"
mkdir -p "$BACKUP_DIR"
BACKUP_FILE="$BACKUP_DIR/${BINARY_NAME}-backup-$(date +%Y%m%d-%H%M%S)"

if [ -f "$INSTALL_DIR/${BINARY_NAME}" ]; then
    cp "$INSTALL_DIR/${BINARY_NAME}" "$BACKUP_FILE"
    log_success "Backed up current binary to: $BACKUP_FILE"
else
    log_warn "No existing binary found"
fi

# Step 2: Stop service
log_info "Step 2: Stopping service"
if [ "$INSTALL_MODE" = "docker" ]; then
    # Docker mode - stop containers
    if [ -d "$DOCKER_DIR" ]; then
        cd "$DOCKER_DIR" || exit 1
        if docker compose version &>/dev/null 2>&1; then
            docker compose down &>/dev/null || true
        else
            docker-compose down &>/dev/null || true
        fi
        log_success "Docker containers stopped"
    else
        log_warn "Docker directory not found"
    fi
else
    # Native mode - stop systemd service
    if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
        systemctl stop "$SERVICE_NAME"
        log_success "Service stopped"
    else
        log_warn "Service not running"
    fi
fi

# Step 3: Install new binary
log_info "Step 3: Installing new binary"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NEW_BINARY="$SCRIPT_DIR/../bin/${BINARY_NAME}"

if [ ! -f "$NEW_BINARY" ]; then
    log_error "New binary not found at: $NEW_BINARY"
    log_error "Please run this script from the build package directory"
    exit 1
fi

cp "$NEW_BINARY" "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/${BINARY_NAME}"
chown root:root "$INSTALL_DIR/${BINARY_NAME}"
log_success "Binary updated"

# Step 4: Update migrations (if any)
log_info "Step 4: Checking for database migrations"
if [ -d "$SCRIPT_DIR/../migrations" ] && [ -d "$DATA_DIR/migrations" ]; then
    # Copy new migrations
    cp -n "$SCRIPT_DIR/../migrations/"* "$DATA_DIR/migrations/" 2>/dev/null || true
    chown -R "${BINARY_NAME}:${BINARY_NAME}" "$DATA_DIR/migrations"
    log_success "Migrations updated"

    if [ "$INSTALL_MODE" = "native" ]; then
        log_warn "You may need to run database migrations manually"
        log_warn "The service will run migrations automatically on startup"
    fi
fi

# Step 5: Update Docker configuration (if in Docker mode)
if [ "$INSTALL_MODE" = "docker" ]; then
    log_info "Step 5: Checking Docker configuration..."
    if [ -f "$PACKAGE_DIR/docker/docker-compose.yml" ]; then
        cp "$PACKAGE_DIR/docker/docker-compose.yml" "$DOCKER_DIR/"
        log_success "Docker Compose configuration updated"
    fi
fi

# Step 6: Start service
log_info "Step 6: Starting service"
if [ "$INSTALL_MODE" = "docker" ]; then
    # Docker mode
    if [ -d "$DOCKER_DIR" ]; then
        cd "$DOCKER_DIR" || exit 1
        if docker compose version &>/dev/null 2>&1; then
            docker compose up -d
        else
            docker-compose up -d
        fi

        sleep 3
        log_success "Docker containers started"
    fi
else
    # Native mode
    systemctl start "$SERVICE_NAME"
    sleep 2

    if systemctl is-active --quiet "$SERVICE_NAME"; then
        log_success "Service started"
    else
        log_error "Failed to start service"
        systemctl status "$SERVICE_NAME" --no-pager
        log_warn "You can restore the backup with: cp $BACKUP_FILE $INSTALL_DIR/${BINARY_NAME}"
        exit 1
    fi
fi

# Step 7: Cleanup old backups
log_info "Step 7: Cleaning up old backups"
find "$BACKUP_DIR" -name "${BINARY_NAME}-backup-*" -mtime +30 -delete 2>/dev/null || true
log_info "Old backups cleaned up"

# Summary
echo ""
log_success "========================================="
log_success "Update completed successfully!"
log_success "========================================="
echo ""
echo "New version: v$VERSION"
echo "Backup location: $BACKUP_FILE"
echo ""

if [ "$INSTALL_MODE" = "docker" ]; then
    echo "To check status:"
    echo "  cd $DOCKER_DIR && docker-compose ps"
    echo ""
    echo "To view logs:"
    echo "  cd $DOCKER_DIR && docker-compose logs -f"
    echo ""
else
    echo "To check status:"
    echo "  systemctl status $SERVICE_NAME"
    echo ""
    echo "To view logs:"
    echo "  journalctl -u $SERVICE_NAME -f"
    echo ""
fi

echo "To rollback, use:"
if [ "$INSTALL_MODE" = "docker" ]; then
    echo "  1. cd $DOCKER_DIR && docker-compose down"
    echo "  2. cp $BACKUP_FILE $INSTALL_DIR/${BINARY_NAME}"
    echo "  3. cd $DOCKER_DIR && docker-compose up -d"
else
    echo "  systemctl stop $SERVICE_NAME"
    echo "  cp $BACKUP_FILE $INSTALL_DIR/${BINARY_NAME}"
    echo "  systemctl start $SERVICE_NAME"
fi
echo ""
