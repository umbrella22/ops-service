#!/bin/bash
# One-click initialization script for {{BINARY_NAME}}
# This script handles installation, database setup, and service startup

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

BINARY_NAME="{{BINARY_NAME}}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[✓]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    log_error "This script must be run as root or with sudo"
    log_info "Please run: sudo $0"
    exit 1
fi

echo ""
log_info "========================================="
log_info "One-Click Initialization for ${BINARY_NAME}"
log_info "========================================="
echo ""
log_info "This script will:"
log_info "  1. Install the service"
log_info "  2. Setup the database"
log_info "  3. Start the service"
echo ""
read -p "Continue? (Y/n): " -n 1 -r
echo
if [[ $REPLY =~ ^[Nn]$ ]]; then
    log_info "Cancelled by user"
    exit 0
fi
echo ""

# Step 1: Install
log_info "Step 1/4: Installing ${BINARY_NAME}..."
if [ -f "$SCRIPT_DIR/install.sh" ]; then
    bash "$SCRIPT_DIR/install.sh"
else
    log_error "install.sh not found"
    exit 1
fi

# Step 2: Fix localhost to 127.0.0.1 (WSL2 compatibility)
echo ""
log_info "Step 2/4: Checking configuration for WSL2 compatibility..."
CONFIG_FILE="/etc/${BINARY_NAME}/env"
if [ -f "$CONFIG_FILE" ]; then
    if grep -q "@localhost:" "$CONFIG_FILE"; then
        log_warn "Found 'localhost' in database URL (may cause DNS issues in WSL2)"
        log_info "Automatically fixing to 127.0.0.1..."

        # Backup original config
        cp "$CONFIG_FILE" "${CONFIG_FILE}.pre-init-backup"

        # Replace localhost with 127.0.0.1
        sed -i 's/@localhost:/@127.0.0.1:/g' "$CONFIG_FILE"
        log_success "Configuration updated for better compatibility"
    else
        log_success "Configuration already uses 127.0.0.1 or custom host"
    fi
else
    log_warn "Configuration file not found, skipping"
fi

# Step 3: Check database
echo ""
log_info "Step 3/4: Verifying database setup..."

# Check if Docker mode is being used
if [ -d "/etc/${BINARY_NAME}/docker" ]; then
    log_info "Docker installation detected, skipping database check"
    log_warn "Docker containers will handle database setup automatically"
else
    if [ -f "$SCRIPT_DIR/check-db.sh" ]; then
        bash "$SCRIPT_DIR/check-db.sh" || {
            log_warn "Database check had warnings, but continuing..."
        }
    else
        log_warn "check-db.sh not found, skipping database verification"
    fi
fi

# Step 4: Start service
echo ""

# Check if Docker mode is being used
if [ -d "/etc/${BINARY_NAME}/docker" ]; then
    log_info "Step 4/4: Docker installation detected"
    log_info "========================================="
    log_success "Docker Installation completed!"
    log_success "========================================="
    echo ""

    # Display seed data status
    if [ -f "/etc/${BINARY_NAME}/docker/.env" ] && grep -q "SEED=true" "/etc/${BINARY_NAME}/docker/.env"; then
        log_info "Configuration:"
        log_success "  Seed data: ✓ Enabled"
        log_info "    - Demo accounts will be created"
        log_info "    - Sample assets will be loaded"
    fi
    echo ""

    log_info "Quick start:"
    log_info "  1. Review configuration: cat /etc/${BINARY_NAME}/docker/.env"
    log_info "  2. Start services: cd /etc/${BINARY_NAME}/docker && docker-compose up -d"
    log_info "  3. View logs: docker-compose logs -f"
    log_info "  4. Stop services: docker-compose down"
    echo ""

    log_info "Services deployed:"
    log_info "  - PostgreSQL database (port 5432, localhost only)"
    log_info "  - API service (internal, accessible via Nginx)"
    log_info "  - Nginx reverse proxy (ports 80, 443)"
    echo ""

    log_info "Default accounts after first start:"
    log_info "  - admin / Admin123! (Administrator)"
    log_info "  - demo  / Demo123!  (Operator)"
    echo ""
    log_warn "Remember to change default passwords!"
    echo ""
    log_info "To start the services now, run:"
    log_info "  cd /etc/${BINARY_NAME}/docker"
    log_info "  docker-compose up -d"
    echo ""
    log_info "For more information, see the documentation in the docs/ directory."
else
    # Systemd mode
    log_info "Step 4/4: Starting ${BINARY_NAME} service..."
    if [ -f "$SCRIPT_DIR/start.sh" ]; then
        bash "$SCRIPT_DIR/start.sh"
    else
        log_error "start.sh not found"
        exit 1
    fi

    echo ""
    log_success "========================================="
    log_success "Initialization Complete!"
    log_success "========================================="
    echo ""
    log_info "Your ${BINARY_NAME} service is now running!"
    echo ""
    log_info "Next steps:"
    log_info "  - Check status: systemctl status ${BINARY_NAME}"
    log_info "  - View logs: journalctl -u ${BINARY_NAME} -f"
    log_info "  - Run on boot: systemctl enable ${BINARY_NAME}"
    echo ""
fi
