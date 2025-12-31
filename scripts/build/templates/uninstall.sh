#!/bin/bash
# Uninstall script for {{BINARY_NAME}}

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
CONFIG_DIR="/etc/${BINARY_NAME}"
DATA_DIR="/var/lib/${BINARY_NAME}"
LOG_DIR="/var/log/${BINARY_NAME}"
SERVICE_NAME="${BINARY_NAME}"

if [ "$EUID" -ne 0 ]; then
    log_error "This script must be run as root or with sudo"
    exit 1
fi

log_info "Uninstalling ${BINARY_NAME} v${VERSION}"
echo ""

# Step 1: Stop and disable service
log_info "Step 1: Stopping and disabling service"
if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
    systemctl stop "$SERVICE_NAME"
    log_success "Service stopped"
fi

if systemctl is-enabled --quiet "$SERVICE_NAME" 2>/dev/null; then
    systemctl disable "$SERVICE_NAME"
    log_success "Service disabled"
fi

# Step 2: Remove systemd service file
log_info "Step 2: Removing systemd service file"
if [ -f "/etc/systemd/system/${SERVICE_NAME}.service" ]; then
    rm -f "/etc/systemd/system/${SERVICE_NAME}.service"
    systemctl daemon-reload
    log_success "Service file removed"
else
    log_warn "Service file not found"
fi

# Step 3: Remove binary
log_info "Step 3: Removing binary"
if [ -f "$INSTALL_DIR/${BINARY_NAME}" ]; then
    rm -f "$INSTALL_DIR/${BINARY_NAME}"
    log_success "Binary removed"
else
    log_warn "Binary not found"
fi

# Step 4: Remove all directories
echo ""
log_info "Step 4: Removing all directories and files"

# Configuration
if [ -d "$CONFIG_DIR" ]; then
    rm -rf "$CONFIG_DIR"
    log_success "Configuration directory removed"
else
    log_warn "Configuration directory not found"
fi

# Data
if [ -d "$DATA_DIR" ]; then
    rm -rf "$DATA_DIR"
    log_success "Data directory removed"
else
    log_warn "Data directory not found"
fi

# Logs
if [ -d "$LOG_DIR" ]; then
    rm -rf "$LOG_DIR"
    log_success "Log directory removed"
else
    log_warn "Log directory not found"
fi

# Step 5: Remove user
echo ""
log_info "Step 5: Removing system user"
if id "$BINARY_NAME" &>/dev/null; then
    userdel "$BINARY_NAME" 2>/dev/null || true
    log_success "User removed"
else
    log_warn "User not found"
fi

# Summary
echo ""
log_success "========================================="
log_success "Uninstall Complete!"
log_success "========================================="
echo ""
log_info "All ${BINARY_NAME} components have been removed:"
echo "  ✓ Service stopped and disabled"
echo "  ✓ Service file removed"
echo "  ✓ Binary removed"
echo "  ✓ Configuration removed"
echo "  ✓ Data removed"
echo "  ✓ Logs removed"
echo "  ✓ System user removed"
echo ""
