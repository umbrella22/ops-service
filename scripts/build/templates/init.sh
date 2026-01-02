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
PACKAGE_DIR="$(dirname "$SCRIPT_DIR")"
DOCKER_DIR="$PACKAGE_DIR/docker"

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
log_info "  1. Check system environment"
log_info "  2. Select installation mode"
log_info "  3. Install the service"
log_info "  4. Setup the database/configuration"
log_info "  5. Start the service"
echo ""

# Check if Docker is available
DOCKER_AVAILABLE=false
if command -v docker &> /dev/null && docker --version &> /dev/null; then
    # Check if Docker daemon is running
    if docker info &> /dev/null 2>&1; then
        DOCKER_AVAILABLE=true
    fi
fi

# Select installation mode
INSTALL_MODE=""
if [ "$DOCKER_AVAILABLE" = true ]; then
    echo ""
    log_info "Docker is detected on your system."
    echo ""
    echo "Installation modes:"
    echo "  1. Docker mode (Recommended)"
    echo "     - All services run in containers"
    echo "     - Easier to manage and upgrade"
    echo "     - Isolated environment"
    echo ""
    echo "  2. Native mode"
    echo "     - Runs as systemd service"
    echo "     - Requires local PostgreSQL"
    echo "     - More manual configuration"
    echo ""
    read -p "Select installation mode [1/2] (default: 1): " -n 1 -r MODE_CHOICE
    echo ""

    if [[ -z "$MODE_CHOICE" ]] || [[ "$MODE_CHOICE" == "1" ]]; then
        INSTALL_MODE="docker"
        log_success "Docker mode selected"
    else
        INSTALL_MODE="native"
        log_success "Native mode selected"
    fi
else
    log_warn "Docker is not available or not running"
    log_info "Using native mode installation"
    INSTALL_MODE="native"
    echo ""
fi

# Confirm installation
echo ""
read -p "Continue with ${INSTALL_MODE} mode installation? (Y/n): " -n 1 -r
echo
if [[ $REPLY =~ ^[Nn]$ ]]; then
    log_info "Cancelled by user"
    exit 0
fi
echo ""

# Ask about seed data
LOAD_SEED_DATA=""
echo ""
log_info "Do you want to load seed data?"
echo "  Seed data includes:"
echo "    - Demo user account (demo/Demo123!)"
echo "    - Test user accounts (john.doe, jane.smith, bob.wilson)"
echo "    - Sample asset groups (dev/stage/prod)"
echo "    - Sample hosts (8 example hosts)"
echo "    - Sample audit logs and login events"
echo ""
echo -n "Load seed data? [y/N] "
read -r response
if [[ "$response" =~ ^[Yy]$ ]]; then
    LOAD_SEED_DATA="--seed-data"
    log_info "Seed data will be loaded"
else
    LOAD_SEED_DATA="--no-seed-data"
    log_info "Seed data will NOT be loaded"
fi
echo ""

# Run installation
log_info "========================================="
log_info "Installing ${BINARY_NAME} (${INSTALL_MODE} mode)"
log_info "========================================="
echo ""

if [ -f "$SCRIPT_DIR/install.sh" ]; then
    if [ "$INSTALL_MODE" = "docker" ]; then
        bash "$SCRIPT_DIR/install.sh" --docker $LOAD_SEED_DATA
    else
        bash "$SCRIPT_DIR/install.sh" --native $LOAD_SEED_DATA
    fi
else
    log_error "install.sh not found"
    exit 1
fi

# Post-installation steps
echo ""
log_info "========================================="
log_info "Post-Installation Setup"
log_info "========================================="
echo ""

if [ "$INSTALL_MODE" = "docker" ]; then
    # Docker mode - show quick start
    log_success "Docker Installation completed!"
    echo ""

    log_info "Quick start:"
    log_info "  1. Review configuration: cat docker/.env"
    log_info "  2. Start services: cd docker && docker-compose up -d"
    log_info "  3. View logs: docker-compose logs -f"
    log_info "  4. Check status: ./scripts/status.sh"
    echo ""

    log_info "To start the services now, run:"
    log_info "  cd docker"
    log_info "  docker-compose up -d"
    echo ""

    # Ask if user wants to start now
    read -p "Start Docker services now? (Y/n): " -n 1 -r
    echo ""
    if [[ ! $REPLY =~ ^[Nn]$ ]]; then
        cd "$DOCKER_DIR" || exit 1
        if docker compose version &>/dev/null; then
            docker compose up -d
        else
            docker-compose up -d
        fi

        sleep 3

        log_success "Services started!"
        echo ""
        log_info "Useful commands:"
        log_info "  - View logs: cd docker && docker-compose logs -f"
        log_info "  - Stop services: ./scripts/stop.sh"
        log_info "  - Restart services: ./scripts/restart.sh"
        log_info "  - Check status: ./scripts/status.sh"
        echo ""
    fi
else
    # Native mode - run start script
    log_info "Starting ${BINARY_NAME} service..."

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
    log_info "  - Enable on boot: systemctl enable ${BINARY_NAME}"
    echo ""
fi

echo ""
log_info "========================================="
log_info "Useful Scripts"
log_info "========================================="
echo ""
log_info "Available management scripts:"
log_info "  - ./scripts/start.sh       Start the service"
log_info "  - ./scripts/stop.sh        Stop the service"
log_info "  - ./scripts/restart.sh     Restart the service"
log_info "  - ./scripts/status.sh      Check service status"
log_info "  - ./scripts/backup.sh      Backup configuration and data"
log_info "  - ./scripts/update.sh      Update to new version"
log_info "  - ./scripts/uninstall.sh   Uninstall the service"
echo ""
log_info "For more information, see the documentation in the docs/ directory."
echo ""
