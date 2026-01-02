#!/bin/bash
# Clean installation script for {{BINARY_NAME}}
# WARNING: This script will remove all existing data, configurations, and the service

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

BINARY_NAME="{{BINARY_NAME}}"
VERSION="{{VERSION}}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGE_DIR="$(dirname "$SCRIPT_DIR")"
DOCKER_DIR="$PACKAGE_DIR/docker"
MARKER_FILE="$PACKAGE_DIR/.docker-mode"
CONFIG_DIR="/etc/${BINARY_NAME}"

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

# Display warning header
clear
echo ""
echo -e "${RED}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${RED}║${NC} ${BOLD}⚠️  CLEAN INSTALLATION WARNING${NC}                           ${RED}║${NC}"
echo -e "${RED}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""

if [ "$INSTALL_MODE" = "docker" ]; then
    echo -e "${YELLOW}${BOLD}Docker Mode Clean Installation${NC}"
    echo ""
    echo -e "${RED}This will:${NC}"
    echo "  1. Stop and remove all Docker containers"
    echo "  2. Remove Docker volumes (ALL DATA WILL BE LOST!)"
    echo "  3. Remove Docker configuration"
    echo "  4. Perform a fresh installation"
    echo "  5. Start fresh Docker containers"
    echo ""
    echo -e "${GREEN}${BOLD}What will be preserved:${NC}"
    echo "  ✅ Docker images"
    echo ""
else
    echo -e "${RED}${BOLD}THIS WILL PERMANENTLY DELETE:${NC}"
    echo ""
    echo -e "${YELLOW}1. All service data in /var/lib/${BINARY_NAME}${NC}"
    echo -e "${YELLOW}2. All configuration files in /etc/${BINARY_NAME}${NC}"
    echo -e "${YELLOW}3. All log files in /var/log/${BINARY_NAME}${NC}"
    echo -e "${YELLOW}4. Systemd service: ${BINARY_NAME}.service${NC}"
    echo -e "${YELLOW}5. System user: ${BINARY_NAME}${NC}"
    echo -e "${YELLOW}6. Binary from /usr/local/bin/${BINARY_NAME}${NC}"
    echo ""
    echo -e "${RED}${BOLD}⚠️  DATABASE WARNING:${NC}"
    echo -e "${RED}This script will also DROP the PostgreSQL database if it exists!${NC}"
    echo ""
    echo -e "${BOLD}What will happen:${NC}"
    echo "  1. Stop and remove the service"
    echo "  2. Remove all files and configurations"
    echo "  3. Drop the database (if it exists)"
    echo "  4. Remove the system user"
    echo "  5. Perform a fresh installation"
    echo "  6. Create a new database"
    echo "  7. Start the service"
    echo ""
    echo -e "${GREEN}${BOLD}What will be preserved:${NC}"
    echo "  ✅ PostgreSQL server installation"
    echo "  ✅ Other databases (only ${BINARY_NAME}'s database will be dropped)"
    echo ""
fi

echo -e "${BOLD}────────────────────────────────────────────────────────────${NC}"
echo ""

# First confirmation
echo -e "${RED}${BOLD}Type 'DELETE' to confirm you want to proceed:${NC}"
read -p "> " -r CONFIRM1
echo ""

if [ "$CONFIRM1" != "DELETE" ]; then
    log_info "Cancelled by user"
    exit 0
fi

# Second confirmation
echo ""
echo -e "${RED}${BOLD}⚠️  FINAL WARNING:${NC}"
if [ "$INSTALL_MODE" = "docker" ]; then
    echo -e "${RED}All Docker volumes and data will be PERMANENTLY DELETED!${NC}"
else
    echo -e "${RED}The database and all data will be PERMANENTLY DELETED!${NC}"
fi
echo ""
echo -e "${BOLD}To proceed, type 'I UNDERSTAND':${NC}"
read -p "> " -r CONFIRM2
echo ""

if [ "$CONFIRM2" != "I UNDERSTAND" ]; then
    log_info "Cancelled by user"
    exit 0
fi

# Start clean installation process
echo ""
log_info "========================================="
log_info "Starting Clean Installation"
log_info "========================================="
echo ""

if [ "$INSTALL_MODE" = "docker" ]; then
    # Docker mode clean install
    log_info "Step 1: Stopping and removing Docker containers and volumes..."
    if [ -d "$DOCKER_DIR" ]; then
        cd "$DOCKER_DIR" || exit 1

        # Stop containers and remove volumes
        if docker compose version &>/dev/null 2>&1; then
            docker compose down -v &>/dev/null || true
        else
            docker-compose down -v &>/dev/null || true
        fi
        log_success "Docker containers stopped and volumes removed"
    fi

    log_info "Step 2: Removing Docker environment configuration..."
    if [ -f "$DOCKER_DIR/.env" ]; then
        rm -f "$DOCKER_DIR/.env"
        log_success "Docker environment configuration removed"
    fi

    # Detect if there's a systemd service to remove
    if systemctl is-active --quiet "$BINARY_NAME" 2>/dev/null; then
        log_info "Step 3: Removing systemd service..."
        systemctl stop "$BINARY_NAME" 2>/dev/null || true
        systemctl disable "$BINARY_NAME" 2>/dev/null || true
        rm -f "/etc/systemd/system/${BINARY_NAME}.service"
        systemctl daemon-reload
        log_success "Systemd service removed"
    fi

else
    # Native mode clean install
    # Step 1: Stop service
    log_info "Step 1: Stopping service..."
    if systemctl is-active --quiet "$BINARY_NAME" 2>/dev/null; then
        systemctl stop "$BINARY_NAME"
        log_success "Service stopped"
    elif systemctl list-unit-files | grep -q "^${BINARY_NAME}.service"; then
        log_warn "Service was not running"
    else
        log_warn "Service not installed"
    fi

    # Step 2: Disable and remove service
    log_info "Step 2: Removing systemd service..."
    if [ -f "/etc/systemd/system/${BINARY_NAME}.service" ]; then
        systemctl disable "$BINARY_NAME" 2>/dev/null || true
        rm -f "/etc/systemd/system/${BINARY_NAME}.service"
        systemctl daemon-reload
        log_success "Service removed"
    else
        log_warn "Service file not found, skipping"
    fi

    # Step 3: Backup database URL before removing config
    log_info "Step 3: Extracting database info..."
    DB_URL=""
    if [ -f "/etc/${BINARY_NAME}/env" ]; then
        DB_URL=$(grep "^OPS_DATABASE__URL=" "/etc/${BINARY_NAME}/env" | cut -d'=' -f2- || echo "")
    fi

    # Step 4: Drop database
    echo ""
    log_info "Step 4: Dropping database..."
    if [ -n "$DB_URL" ]; then
        # Parse database name
        DB_NAME=$(echo "$DB_URL" | sed -n 's|.*/\(.*\)|\1|p')

        if [ -n "$DB_NAME" ]; then
            log_warn "Attempting to drop database '$DB_NAME'..."

            # Check if PostgreSQL is running
            if systemctl is-active --quiet postgresql 2>/dev/null; then
                # Drop database
                if sudo -u postgres psql -c "DROP DATABASE IF EXISTS $DB_NAME;" &> /dev/null; then
                    log_success "Database '$DB_NAME' dropped"
                else
                    log_warn "Failed to drop database (may not exist or permission denied)"
                    log_warn "You may need to drop it manually:"
                    log_warn "  sudo -u postgres psql"
                    log_warn "  DROP DATABASE $DB_NAME;"
                fi
            else
                log_warn "PostgreSQL is not running, skipping database drop"
            fi
        fi
    else
        log_warn "Could not extract database URL, skipping database drop"
    fi
    echo ""

    # Step 5: Remove configuration files
    log_info "Step 5: Removing configuration files..."
    if [ -d "/etc/${BINARY_NAME}" ]; then
        rm -rf "/etc/${BINARY_NAME}"
        log_success "Configuration directory removed"
    else
        log_warn "Configuration directory not found, skipping"
    fi

    # Step 6: Remove data files
    log_info "Step 6: Removing data files..."
    if [ -d "/var/lib/${BINARY_NAME}" ]; then
        rm -rf "/var/lib/${BINARY_NAME}"
        log_success "Data directory removed"
    else
        log_warn "Data directory not found, skipping"
    fi

    # Step 7: Remove log files
    log_info "Step 7: Removing log files..."
    if [ -d "/var/log/${BINARY_NAME}" ]; then
        rm -rf "/var/log/${BINARY_NAME}"
        log_success "Log directory removed"
    else
        log_warn "Log directory not found, skipping"
    fi

    # Step 8: Remove binary
    log_info "Step 8: Removing binary..."
    if [ -f "/usr/local/bin/${BINARY_NAME}" ]; then
        rm -f "/usr/local/bin/${BINARY_NAME}"
        log_success "Binary removed"
    else
        log_warn "Binary not found, skipping"
    fi

    # Step 9: Remove system user
    log_info "Step 9: Removing system user..."
    if id "$BINARY_NAME" &>/dev/null; then
        userdel "$BINARY_NAME" 2>/dev/null || true
        log_success "System user removed"
    else
        log_warn "System user not found, skipping"
    fi
fi

# Step 10: Run normal installation
echo ""
log_info "========================================="
log_info "Running Fresh Installation"
log_info "========================================="
echo ""

if [ -f "$SCRIPT_DIR/install.sh" ]; then
    if [ "$INSTALL_MODE" = "docker" ]; then
        bash "$SCRIPT_DIR/install.sh" --docker
    else
        bash "$SCRIPT_DIR/install.sh" --native
    fi
else
    log_error "install.sh not found in $SCRIPT_DIR"
    exit 1
fi

# Step 11: Start service
echo ""
log_info "========================================="
log_info "Starting Service"
log_info "========================================="
echo ""

if [ -f "$SCRIPT_DIR/start.sh" ]; then
    bash "$SCRIPT_DIR/start.sh"
else
    log_error "start.sh not found in $SCRIPT_DIR"
    if [ "$INSTALL_MODE" = "native" ]; then
        log_info "Please start the service manually:"
        log_info "  sudo systemctl start ${BINARY_NAME}"
    else
        log_info "Please start the service manually:"
        log_info "  cd docker && docker-compose up -d"
    fi
    exit 1
fi

# Summary
echo ""
log_success "========================================="
log_success "Clean Installation Complete!"
log_success "========================================="
echo ""
log_info "Your ${BINARY_NAME} service is now running with a fresh installation."
echo ""

if [ "$INSTALL_MODE" = "docker" ]; then
    log_info "Next steps:"
    log_info "  - View logs: cd docker && docker-compose logs -f"
    log_info "  - Check status: ./scripts/status.sh"
else
    log_info "Next steps:"
    log_info "  - Check status: systemctl status ${BINARY_NAME}"
    log_info "  - View logs: journalctl -u ${BINARY_NAME} -f"
    log_info "  - Edit config: nano /etc/${BINARY_NAME}/env"
fi
echo ""
