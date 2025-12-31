#!/bin/bash
# Start script for {{BINARY_NAME}}

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

SERVICE_NAME="{{BINARY_NAME}}"
CONFIG_DIR="/etc/${SERVICE_NAME}"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[✓]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    log_error "This script must be run as root or with sudo"
    exit 1
fi

# Header
echo ""
log_info "========================================="
log_info "Starting ${SERVICE_NAME} service"
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
# Remove protocol prefix
DB_URL_CLEAN="${DB_URL#postgresql://}"
DB_URL_CLEAN="${DB_URL_CLEAN#postgres://}"

# Extract username and password (everything before @)
DB_CREDENTIALS="${DB_URL_CLEAN%@*}"
DB_HOST_PORT="${DB_URL_CLEAN#*@}"

# Extract host and port
if [[ "$DB_HOST_PORT" == *:* ]]; then
    DB_HOST="${DB_HOST_PORT%:*}"
    DB_PORT_PART="${DB_HOST_PORT#*:}"
    # Extract database name from port part
    DB_NAME="${DB_PORT_PART#*/}"
    # Get port number
    DB_PORT="${DB_PORT_PART%/*}"
else
    DB_HOST="$DB_HOST_PORT"
    DB_NAME="${DB_HOST_PORT#*/}"
    DB_PORT="5432"
fi

# Fallback to localhost if host is empty
if [ -z "$DB_HOST" ]; then
    DB_HOST="localhost"
fi

# Fallback to 5432 if port is not a number
if ! [[ "$DB_PORT" =~ ^[0-9]+$ ]]; then
    DB_PORT="5432"
fi

log_success "Database: $DB_HOST:$DB_PORT/$DB_NAME"

# Step 5: Check if PostgreSQL is running
log_info "Step 5: Checking PostgreSQL service..."
if ! systemctl is-active --quiet postgresql && ! systemctl is-active --quiet postgresql@* 2>/dev/null; then
    log_warn "PostgreSQL service does not appear to be running"
    log_warn "Trying to detect database connection anyway..."
fi

# Step 6: Test database connection
log_info "Step 6: Testing database connection..."

# Check if psql is available
if command -v psql &> /dev/null; then
    # Try to connect with timeout
    if timeout 5 psql "$DB_URL" -c "SELECT 1" &> /dev/null; then
        log_success "Database connection successful"
    else
        log_error "Failed to connect to database"
        echo ""
        log_warn "Attempting to create database '$DB_NAME'..."

        # Try to create database automatically
        if sudo -u postgres psql -c "CREATE DATABASE $DB_NAME;" &> /dev/null; then
            log_success "Database '$DB_NAME' created successfully"

            # Try connection again
            if timeout 5 psql "$DB_URL" -c "SELECT 1" &> /dev/null; then
                log_success "Database connection successful"
            else
                log_warn "Database created but connection still fails"
                log_warn "This might be due to authentication settings"
                echo ""
                log_warn "Please ensure:"
                log_warn "  1. PostgreSQL is running: systemctl status postgresql"
                log_warn "  2. Database credentials in $CONFIG_DIR/env are correct"
                log_warn "  3. PostgreSQL accepts connections (check pg_hba.conf)"
                echo ""
                read -p "Continue anyway? (y/N): " -n 1 -r
                echo
                if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                    log_info "Startup cancelled"
                    exit 1
                fi
            fi
        else
            log_error "Could not create database automatically"
            echo ""
            log_warn "Please ensure:"
            log_warn "  1. PostgreSQL is running: systemctl status postgresql"
            log_warn "  2. Database '$DB_NAME' exists (or can be created)"
            log_warn "  3. Database credentials in $CONFIG_DIR/env are correct"
            log_warn "  4. PostgreSQL accepts connections (check pg_hba.conf)"
            echo ""
            log_warn "To create the database manually:"
            log_warn "  sudo -u postgres psql"
            log_warn "  CREATE DATABASE $DB_NAME;"
            log_warn "  \\q"
            echo ""
            read -p "Continue anyway? (y/N): " -n 1 -r
            echo
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                log_info "Startup cancelled"
                exit 1
            fi
        fi
    fi
else
    log_warn "psql command not found, skipping database connection test"
    log_warn "Installing postgresql-client is recommended"
fi

# Step 7: Check if migration files exist
log_info "Step 7: Checking migration files..."
MIGRATION_DIR="/var/lib/${SERVICE_NAME}/migrations"
if [ -d "$MIGRATION_DIR" ] && [ "$(ls -A $MIGRATION_DIR 2>/dev/null)" ]; then
    MIGRATION_COUNT=$(ls -1 "$MIGRATION_DIR"/*.sql 2>/dev/null | wc -l)
    log_success "Found $MIGRATION_COUNT migration file(s)"
else
    log_warn "No migration files found in $MIGRATION_DIR"
fi

# Step 8: Stop service if already running
log_info "Step 8: Checking service status..."
if systemctl is-active --quiet "$SERVICE_NAME"; then
    log_warn "Service is already running, restarting..."
    systemctl stop "$SERVICE_NAME"
    sleep 2
fi

# Step 9: Reload systemd
log_info "Step 9: Reloading systemd daemon..."
systemctl daemon-reload
log_success "Systemd reloaded"

# Step 10: Start the service
echo ""
log_info "Step 10: Starting ${SERVICE_NAME} service..."
echo ""

systemctl start "$SERVICE_NAME"

# Wait for service to start
sleep 3

# Step 11: Check service status
echo ""
log_info "Step 11: Checking service status..."
echo ""

if systemctl is-active --quiet "$SERVICE_NAME"; then
    log_success "Service started successfully!"
    echo ""
    echo "Service details:"
    systemctl status "$SERVICE_NAME" --no-pager -l
    echo ""
    log_info "Service is running at:"
    log_info "  - Logs: journalctl -u ${SERVICE_NAME} -f"
    log_info "  - Status: systemctl status ${SERVICE_NAME}"
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
    systemctl status "$SERVICE_NAME" --no-pager -l
    echo ""
    log_error "Recent logs:"
    journalctl -u "${SERVICE_NAME}" -n 50 --no-pager
    echo ""
    log_error "Troubleshooting:"
    log_error "  1. Check logs: journalctl -u ${SERVICE_NAME} -f"
    log_error "  2. Check database: psql $DB_URL"
    log_error "  3. Check config: cat $CONFIG_DIR/env"
    log_error "  4. Check database migrations: ls -la $MIGRATION_DIR"
    echo ""
    exit 1
fi
