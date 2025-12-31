#!/bin/bash
# Database check and initialization script for {{BINARY_NAME}}

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

SERVICE_NAME="{{BINARY_NAME}}"
CONFIG_DIR="/etc/${SERVICE_NAME}}"
DATA_DIR="/var/lib/${SERVICE_NAME}"

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[✓]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    if command -v sudo &> /dev/null; then
        log_warn "This script requires root privileges, restarting with sudo..."
        exec sudo bash "$0" "$@"
    else
        log_error "This script must be run as root"
        exit 1
    fi
fi

echo ""
log_info "========================================="
log_info "Database Check for ${SERVICE_NAME}"
log_info "========================================="
echo ""

# Step 1: Check configuration file
log_info "Step 1: Loading configuration..."
if [ ! -f "$CONFIG_DIR/env" ]; then
    log_error "Configuration file not found: $CONFIG_DIR/env"
    exit 1
fi

# Load database URL
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
DB_USER="${DB_URL_CLEAN%:*}"
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

# Fallbacks
if [ -z "$DB_HOST" ]; then
    DB_HOST="localhost"
fi
if [ -z "$DB_PORT" ] || ! [[ "$DB_PORT" =~ ^[0-9]+$ ]]; then
    DB_PORT="5432"
fi

log_success "Configuration loaded"
log_info "  Host: $DB_HOST"
log_info "  Port: $DB_PORT"
log_info "  User: $DB_USER"
log_info "  Database: $DB_NAME"
echo ""

# Step 2: Check PostgreSQL service
log_info "Step 2: Checking PostgreSQL service..."

if systemctl is-active --quiet postgresql 2>/dev/null; then
    log_success "PostgreSQL is running"
elif systemctl is-active --quiet postgresql@* 2>/dev/null; then
    log_success "PostgreSQL is running (cluster service)"
else
    log_error "PostgreSQL is not running"
    log_warn "Start PostgreSQL with: systemctl start postgresql"
    echo ""
    read -p "Continue anyway? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi
echo ""

# Step 3: Test database connection
log_info "Step 3: Testing database connection..."

if ! command -v psql &> /dev/null; then
    log_error "psql command not found"
    log_error "Install PostgreSQL client: apt install postgresql-client"
    exit 1
fi

# Try to connect to PostgreSQL (without specifying database)
if timeout 5 psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d postgres -c "SELECT 1" &> /dev/null; then
    log_success "Can connect to PostgreSQL server"
else
    log_error "Cannot connect to PostgreSQL server"
    echo ""
    log_warn "Possible reasons:"
    log_warn "  1. PostgreSQL is not accepting connections"
    log_warn "  2. Wrong username/password"
    log_warn "  3. Firewall blocking connections"
    log_warn "  4. pg_hba.conf doesn't allow this connection"
    echo ""
    log_warn "Test connection manually:"
    log_warn "  psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d postgres"
    exit 1
fi
echo ""

# Step 4: Check if database exists
log_info "Step 4: Checking if database '$DB_NAME' exists..."

DB_EXISTS=$(psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d postgres -tAc "SELECT 1 FROM pg_database WHERE datname='$DB_NAME'")

if [ "$DB_EXISTS" = "1" ]; then
    log_success "Database '$DB_NAME' exists"
else
    log_warn "Database '$DB_NAME' does not exist"
    echo ""
    log_info "Creating database '$DB_NAME'..."
    if psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d postgres -c "CREATE DATABASE $DB_NAME;" &> /dev/null; then
        log_success "Database created successfully"
    else
        log_error "Failed to create database"
        log_error "You may need to create it manually:"
        log_error "  sudo -u postgres psql"
        log_error "  CREATE DATABASE $DB_NAME OWNER $DB_USER;"
        log_error "  \\q"
        exit 1
    fi
fi
echo ""

# Step 5: Test connection to the target database
log_info "Step 5: Testing connection to database '$DB_NAME'..."

if timeout 5 psql "$DB_URL" -c "SELECT 1" &> /dev/null; then
    log_success "Can connect to database '$DB_NAME'"
else
    log_error "Cannot connect to database '$DB_NAME'"
    exit 1
fi
echo ""

# Step 6: Check if migrations table exists
log_info "Step 6: Checking migration status..."

MIGRATION_TABLE_EXISTS=$(psql "$DB_URL" -tAc "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_schema = 'public' AND table_name = '_sqlx_migrations')")

if [ "$MIGRATION_TABLE_EXISTS" = "t" ]; then
    log_success "Migration table exists"
    APPLIED_MIGRATIONS=$(psql "$DB_URL" -tAc "SELECT COUNT(*) FROM _sqlx_migrations")
    log_info "  Applied migrations: $APPLIED_MIGRATIONS"
else
    log_warn "No migrations applied yet"
    log_info "  Migrations will be applied when the service starts"
fi
echo ""

# Step 7: Check migration files
log_info "Step 7: Checking migration files..."
MIGRATION_DIR="$DATA_DIR/migrations"

if [ -d "$MIGRATION_DIR" ]; then
    MIGRATION_FILES=$(ls -1 "$MIGRATION_DIR"/*.sql 2>/dev/null | wc -l)
    log_success "Found $MIGRATION_FILES migration file(s) in $MIGRATION_DIR"

    if [ "$MIGRATION_FILES" -gt 0 ]; then
        log_info "  Migration files:"
        ls -1 "$MIGRATION_DIR"/*.sql 2>/dev/null | while read -r file; do
            log_info "    - $(basename "$file")"
        done
    fi
else
    log_warn "Migration directory not found: $MIGRATION_DIR"
fi
echo ""

# Step 8: Test basic query
log_info "Step 8: Testing database operations..."

if psql "$DB_URL" -c "SELECT version();" &> /dev/null; then
    log_success "Database is responding to queries"
    PG_VERSION=$(psql "$DB_URL" -tAc "SELECT version()" | head -1)
    log_info "  PostgreSQL: $PG_VERSION"
else
    log_error "Database not responding to queries"
    exit 1
fi
echo ""

# Step 9: Summary
log_success "========================================="
log_success "Database Check Complete!"
log_success "========================================="
echo ""
log_info "Summary:"
log_info "  ✓ Configuration is valid"
log_info "  ✓ PostgreSQL is running"
log_info "  ✓ Database '$DB_NAME' exists"
log_info "  ✓ Connection successful"
echo ""

if [ "$MIGRATION_TABLE_EXISTS" = "t" ]; then
    log_info "Next steps:"
    log_info "  1. Start the service: systemctl start $SERVICE_NAME"
    log_info "  2. Check status: systemctl status $SERVICE_NAME"
    log_info "  3. View logs: journalctl -u $SERVICE_NAME -f"
else
    log_info "Next steps:"
    log_info "  1. Start the service (this will apply migrations):"
    log_info "     systemctl start $SERVICE_NAME"
    log_info "  2. Check status: systemctl status $SERVICE_NAME"
    log_info "  3. View logs: journalctl -u $SERVICE_NAME -f"
fi
echo ""

exit 0
