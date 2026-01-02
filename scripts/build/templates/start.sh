#!/bin/bash
# Start script for {{BINARY_NAME}}

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
MARKER_FILE="$PACKAGE_DIR/.docker-mode"

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[✓]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

# Check if running as root
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
    echo ""
    log_info "========================================="
    log_info "Starting ${BINARY_NAME} (Docker mode)"
    log_info "========================================="
    echo ""

    if [ ! -d "$DOCKER_DIR" ]; then
        log_error "Docker directory not found: $DOCKER_DIR"
        log_error "Please run install.sh first"
        exit 1
    fi

    cd "$DOCKER_DIR" || exit 1

    log_info "Starting Docker containers..."
    echo ""

    # Check which docker-compose command is available
    if docker compose version &>/dev/null 2>&1; then
        docker compose up -d
    else
        docker-compose up -d
    fi

    echo ""
    log_success "Docker containers started successfully!"
    echo ""
    log_info "Useful commands:"
    log_info "  - View logs: cd $DOCKER_DIR && docker-compose logs -f"
    log_info "  - Check status: ./scripts/status.sh"
    log_info "  - Stop services: ./scripts/stop.sh"
    echo ""

else
    # Native mode (systemd)
    SERVICE_FILE="/etc/systemd/system/${BINARY_NAME}.service"
    CONFIG_DIR="/etc/${BINARY_NAME}"

    echo ""
    log_info "========================================="
    log_info "Starting ${BINARY_NAME} (Native mode)"
    log_info "========================================="
    echo ""

    # Step 1: Check if systemd service file exists
    log_info "Step 1: Checking systemd service..."
    if [ ! -f "$SERVICE_FILE" ]; then
        log_error "Systemd service file not found: $SERVICE_FILE"
        log_error "Please run the install script first"
        exit 1
    fi
    log_success "Systemd service file found"

    # Step 2: Check configuration file
    log_info "Step 2: Checking configuration file..."
    if [ ! -f "$CONFIG_DIR/env" ]; then
        log_error "Configuration file not found: $CONFIG_DIR/env"
        log_error "Please run the install script first"
        exit 1
    fi
    log_success "Configuration file found"

    # Step 3: Validate configuration format
    log_info "Step 3: Validating configuration format..."
    if ! grep -q "OPS_DATABASE__URL=" "$CONFIG_DIR/env" 2>/dev/null; then
        log_error "Invalid configuration format!"
        echo ""
        log_warn "The configuration file must use double underscore (__) for nested fields"
        log_warn "Correct format: OPS_DATABASE__URL=postgresql://..."
        log_warn "Wrong format: OPS_DATABASE_URL=postgresql://..."
        echo ""
        log_warn "Please update your configuration file: $CONFIG_DIR/env"
        exit 1
    fi
    log_success "Configuration format is valid"

    # Step 4: Extract database URL from config
    log_info "Step 4: Extracting database configuration..."
    DB_URL=$(grep "^OPS_DATABASE__URL=" "$CONFIG_DIR/env" | cut -d'=' -f2-)

    if [ -z "$DB_URL" ]; then
        log_error "Database URL not found in configuration"
        log_error "Please set OPS_DATABASE__URL in $CONFIG_DIR/env"
        exit 1
    fi

    # Parse database connection details
    DB_URL_CLEAN="${DB_URL#postgresql://}"
    DB_URL_CLEAN="${DB_URL_CLEAN#postgres://}"
    DB_HOST_PORT="${DB_URL_CLEAN#*@}"

    # Parse database name
    if [[ "$DB_HOST_PORT" == */* ]]; then
        DB_NAME="${DB_HOST_PORT#*/}"
    else
        DB_NAME=""
    fi

    log_success "Database: $DB_NAME"

    # Step 5: Check if PostgreSQL is running
    log_info "Step 5: Checking PostgreSQL service..."
    if ! systemctl is-active --quiet postgresql && ! systemctl is-active --quiet postgresql@* 2>/dev/null; then
        log_warn "PostgreSQL service does not appear to be running"
        log_warn "Trying to detect database connection anyway..."
    fi

    # Step 6: Test database connection (optional)
    log_info "Step 6: Testing database connection..."

    if command -v psql &> /dev/null; then
        if timeout 5 psql "$DB_URL" -c "SELECT 1" &> /dev/null 2>&1; then
            log_success "Database connection successful"
        else
            log_warn "Could not connect to database"
            log_warn "The service will attempt to connect on startup"
        fi
    else
        log_warn "psql command not found, skipping database connection test"
    fi

    # Step 7: Stop service if already running
    log_info "Step 7: Checking service status..."
    if systemctl is-active --quiet "$BINARY_NAME"; then
        log_warn "Service is already running, restarting..."
        systemctl stop "$BINARY_NAME"
        sleep 2
    fi

    # Step 8: Reload systemd
    log_info "Step 8: Reloading systemd daemon..."
    systemctl daemon-reload
    log_success "Systemd reloaded"

    # Step 9: Start the service
    echo ""
    log_info "Step 9: Starting ${BINARY_NAME} service..."
    echo ""

    systemctl start "$BINARY_NAME"

    # Wait for service to start
    sleep 3

    # Step 10: Check service status
    echo ""
    log_info "Step 10: Checking service status..."
    echo ""

    if systemctl is-active --quiet "$BINARY_NAME"; then
        log_success "Service started successfully!"
        echo ""
        echo "Service details:"
        systemctl status "$BINARY_NAME" --no-pager -l
        echo ""
        log_info "Service is running at:"
        log_info "  - Logs: journalctl -u ${BINARY_NAME} -f"
        log_info "  - Status: systemctl status ${BINARY_NAME}"
        echo ""

        # Try to show the listening port
        ADDR=$(grep "^OPS_SERVER__ADDR=" "$CONFIG_DIR/env" | cut -d'=' -f2)
        if [ -n "$ADDR" ]; then
            log_info "  - Endpoint: http://$ADDR"
        fi
        echo ""

        # Show health check tip
        log_info "To verify the service is working:"
        log_info "  curl http://localhost:3000/api/health"
        echo ""

        exit 0
    else
        log_error "Failed to start service"
        echo ""
        log_error "Error details:"
        systemctl status "$BINARY_NAME" --no-pager -l
        echo ""
        log_error "Recent logs:"
        journalctl -u "${BINARY_NAME}" -n 50 --no-pager
        echo ""
        log_error "Troubleshooting:"
        log_error "  1. Check logs: journalctl -u ${BINARY_NAME} -f"
        log_error "  2. Check database: psql $DB_URL"
        log_error "  3. Check config: cat $CONFIG_DIR/env"
        echo ""
        exit 1
    fi
fi
