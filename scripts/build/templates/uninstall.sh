#!/bin/bash
# Uninstall script for {{BINARY_NAME}}

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
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
CONFIG_DIR="/etc/${BINARY_NAME}"
DATA_DIR="/var/lib/${BINARY_NAME}"
LOG_DIR="/var/log/${BINARY_NAME}"
SERVICE_NAME="${BINARY_NAME}"

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

echo ""
log_info "========================================="
log_info "Uninstalling ${BINARY_NAME} v${VERSION}"
log_info "========================================="
echo ""
log_info "Detected installation mode: ${INSTALL_MODE}"
echo ""

# Show warning
if [ "$INSTALL_MODE" = "docker" ]; then
    echo -e "${YELLOW}${BOLD}Docker Mode Uninstallation${NC}"
    echo ""
    echo "This will:"
    echo "  1. Stop and remove Docker containers"
    echo "  2. Remove the Docker mode marker file"
    echo "  3. Remove systemd service (if exists)"
    echo "  4. Remove binary and user"
    echo ""
    echo -e "${GREEN}All files in $PACKAGE_DIR will be preserved${NC}"
    echo -e "${YELLOW}Note: Docker volumes will be preserved unless you remove them manually.${NC}"
else
    echo -e "${RED}${BOLD}Native Mode Uninstallation${NC}"
    echo ""
    echo -e "${RED}This will:${NC}"
    echo "  1. Stop and disable the service"
    echo "  2. Remove systemd service file"
    echo "  3. Remove binary from $INSTALL_DIR"
    echo "  4. Remove all configuration from $CONFIG_DIR"
    echo -e "${RED}  5. Remove all data from $DATA_DIR${NC}"
    echo "  6. Remove logs from $LOG_DIR"
    echo "  7. Remove system user $BINARY_NAME"
    echo ""
    echo -e "${RED}${BOLD}⚠️  WARNING: ALL DATA WILL BE PERMANENTLY LOST!${NC}"
fi
echo ""

# First confirmation
read -p "Continue uninstallation? (y/N): " -n 1 -r CONFIRM1
echo ""
if [[ ! $CONFIRM1 =~ ^[Yy]$ ]]; then
    log_info "Cancelled by user"
    exit 0
fi

# Second confirmation for native mode (data loss warning)
if [ "$INSTALL_MODE" = "native" ]; then
    echo ""
    echo -e "${RED}${BOLD}⚠️  FINAL WARNING:${NC}"
    echo -e "${RED}This action cannot be undone!${NC}"
    echo -e "${RED}All data in $DATA_DIR will be permanently deleted!${NC}"
    echo ""
    echo -e "${BOLD}Type 'DELETE' to confirm:${NC}"
    read -p "> " -r CONFIRM2
    echo ""

    if [ "$CONFIRM2" != "DELETE" ]; then
        log_info "Cancelled by user"
        exit 0
    fi
    echo ""
fi

# Start uninstallation
if [ "$INSTALL_MODE" = "docker" ]; then
    # Docker mode uninstallation
    log_info "========================================="
    log_info "Docker Mode Uninstallation"
    log_info "========================================="
    echo ""

    # Step 1: Stop Docker containers
    log_info "Step 1: Stopping Docker containers..."
    if [ -d "$DOCKER_DIR" ]; then
        cd "$DOCKER_DIR" || exit 1

        # Check which docker-compose command is available
        if docker compose version &>/dev/null 2>&1; then
            docker compose down &>/dev/null || true
        else
            docker-compose down &>/dev/null || true
        fi
        log_success "Docker containers stopped"
    else
        log_warn "Docker directory not found"
    fi

    # Step 2: Remove marker file (not the actual directory)
    log_info "Step 2: Removing Docker mode marker..."
    if [ -f "$MARKER_FILE" ]; then
        rm -f "$MARKER_FILE"
        log_success "Docker mode marker removed"
    else
        log_warn "Marker file not found"
    fi

    # Step 3: Remove systemd service (if exists)
    log_info "Step 3: Removing systemd service (if exists)..."
    if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
        systemctl stop "$SERVICE_NAME"
        log_success "Service stopped"
    fi

    if systemctl is-enabled --quiet "$SERVICE_NAME" 2>/dev/null; then
        systemctl disable "$SERVICE_NAME"
        log_success "Service disabled"
    fi

    if [ -f "/etc/systemd/system/${SERVICE_NAME}.service" ]; then
        rm -f "/etc/systemd/system/${SERVICE_NAME}.service"
        systemctl daemon-reload
        log_success "Service file removed"
    else
        log_warn "Service file not found"
    fi

    # Step 4: Remove binary
    log_info "Step 4: Removing binary..."
    if [ -f "$INSTALL_DIR/${BINARY_NAME}" ]; then
        rm -f "$INSTALL_DIR/${BINARY_NAME}"
        log_success "Binary removed"
    else
        log_warn "Binary not found"
    fi

    # Step 5: Remove user
    log_info "Step 5: Removing system user..."
    if id "$BINARY_NAME" &>/dev/null; then
        userdel "$BINARY_NAME" 2>/dev/null || true
        log_success "User removed"
    else
        log_warn "User not found"
    fi

    # Summary for Docker mode
    echo ""
    log_success "========================================="
    log_success "Docker Uninstallation Complete!"
    log_success "========================================="
    echo ""
    log_info "Removed components:"
    echo "  ✓ Docker containers stopped"
    echo "  ✓ Docker mode marker removed"
    echo "  ✓ Systemd service removed (if existed)"
    echo "  ✓ Binary removed"
    echo "  ✓ System user removed"
    echo ""
    log_success "All files remain in: $PACKAGE_DIR"
    log_info "You can manually delete the directory if needed:"
    log_info "  cd .. && rm -rf $(basename "$PACKAGE_DIR")"
    echo ""
    log_warn "Note: Docker volumes may still exist. To remove them:"
    log_warn "  docker volume ls"
    log_warn "  docker volume rm <volume_name>"
    echo ""

else
    # Native mode uninstallation
    log_info "========================================="
    log_info "Native Mode Uninstallation"
    log_info "========================================="
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

    # Step 4: Remove all directories with data warning
    echo ""
    log_info "Step 4: Removing all directories and files"
    log_warn "This will permanently delete all data..."
    echo ""

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
        log_success "Data directory removed (all data deleted)"
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

    # Summary for Native mode
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
    echo "  ✓ Data removed (all data permanently deleted)"
    echo "  ✓ Logs removed"
    echo "  ✓ System user removed"
    echo ""
fi

log_info "Uninstallation completed successfully."
echo ""
