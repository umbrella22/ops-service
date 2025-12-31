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
INSTALL_DIR="/usr/local/bin"
SERVICE_NAME="${BINARY_NAME}"
BACKUP_DIR="/var/backups/${BINARY_NAME}"

if [ "$EUID" -ne 0 ]; then
    log_error "This script must be run as root or with sudo"
    exit 1
fi

log_info "Updating ${BINARY_NAME} to v${VERSION}"
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
if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
    systemctl stop "$SERVICE_NAME"
    log_success "Service stopped"
else
    log_warn "Service not running"
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
DATA_DIR="/var/lib/${BINARY_NAME}"
if [ -d "$SCRIPT_DIR/../migrations" ] && [ -d "$DATA_DIR/migrations" ]; then
    # Copy new migrations
    cp -n "$SCRIPT_DIR/../migrations/"* "$DATA_DIR/migrations/" 2>/dev/null || true
    log_success "Migrations updated"

    log_warn "You may need to run database migrations manually"
    log_warn "Check the documentation for migration procedures"
fi

# Step 5: Start service
log_info "Step 5: Starting service"
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

# Step 6: Cleanup old backups
log_info "Step 6: Cleaning up old backups"
find "$BACKUP_DIR" -name "${BINARY_NAME}-backup-*" -mtime +30 -delete 2>/dev/null || true
log_info "Old backups cleaned up"

# Summary
echo ""
log_success "Update completed successfully!"
echo ""
echo "New version: v$VERSION"
echo "Backup location: $BACKUP_FILE"
echo ""
echo "To rollback, use:"
echo "  systemctl stop $SERVICE_NAME"
echo "  cp $BACKUP_FILE $INSTALL_DIR/${BINARY_NAME}"
echo "  systemctl start $SERVICE_NAME"
