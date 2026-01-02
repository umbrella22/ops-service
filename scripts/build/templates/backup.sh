#!/bin/bash
# Backup script for {{BINARY_NAME}}

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
CONFIG_DIR="/etc/${BINARY_NAME}"
DATA_DIR="/var/lib/${BINARY_NAME}"
BACKUP_DIR="/var/backups/${BINARY_NAME}"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
BACKUP_NAME="${BINARY_NAME}-backup-${TIMESTAMP}"

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

log_info "Creating backup for ${BINARY_NAME} (${INSTALL_MODE} mode)"
echo ""

# Create backup directory
mkdir -p "$BACKUP_DIR"
cd "$BACKUP_DIR"

# Create temporary directory for this backup
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

# Step 1: Backup configuration
log_info "Step 1: Backing up configuration"
if [ -d "$CONFIG_DIR" ]; then
    if [ "$INSTALL_MODE" = "docker" ]; then
        # For Docker mode, backup the docker directory
        if [ -d "$DOCKER_DIR" ]; then
            cp -r "$DOCKER_DIR" "$TEMP_DIR/docker"
            log_success "Docker configuration backed up"
        fi
    else
        # For Native mode, backup the config directory
        if [ -f "$CONFIG_DIR/env" ]; then
            mkdir -p "$TEMP_DIR/config"
            cp "$CONFIG_DIR/env" "$TEMP_DIR/config/"
            log_success "Configuration backed up"
        fi
    fi
else
    log_warn "No configuration directory found"
fi

# Step 2: Backup data directory (only for native mode)
if [ "$INSTALL_MODE" = "native" ]; then
    log_info "Step 2: Backing up data directory"
    if [ -d "$DATA_DIR" ]; then
        cp -r "$DATA_DIR" "$TEMP_DIR/data"
        log_success "Data directory backed up"
    else
        log_warn "No data directory found"
    fi
else
    log_info "Step 2: Skipping data directory backup (Docker mode uses volumes)"
fi

# Step 3: Backup Docker volumes (Docker mode only)
if [ "$INSTALL_MODE" = "docker" ]; then
    log_info "Step 3: Backing up Docker volumes"

    # Check if Docker is running
    if docker info &> /dev/null 2>&1; then
        # Backup PostgreSQL database from running container
        if [ -f "$DOCKER_DIR/docker-compose.yml" ]; then
            cd "$DOCKER_DIR" || exit 1

            # Try to find the PostgreSQL container
            DB_CONTAINER=$(docker ps --filter "name=postgres" --format "{{.Names}}" | head -n 1)

            if [ -n "$DB_CONTAINER" ]; then
                log_info "Backing up PostgreSQL database from container..."

                # Extract database credentials from .env
                if [ -f "$DOCKER_DIR/.env" ]; then
                    source "$DOCKER_DIR/.env"

                    if [ -n "${POSTGRES_DB:-}" ] && [ -n "${POSTGRES_USER:-}" ]; then
                        if docker exec "$DB_CONTAINER" pg_dump -U "$POSTGRES_USER" "$POSTGRES_DB" > "$TEMP_DIR/database.sql" 2>/dev/null; then
                            log_success "Database backed up"
                        else
                            log_warn "Database backup failed (continuing)"
                        fi
                    fi
                fi
            else
                log_warn "No PostgreSQL container found running"
            fi

            cd "$BACKUP_DIR" || exit 1
        fi
    else
        log_warn "Docker is not running, skipping database backup"
    fi
else
    # Native mode - backup database with pg_dump
    log_info "Step 3: Checking for database backup"
    if command -v pg_dump >/dev/null 2>&1; then
        # Try to extract database URL from config
        if [ -f "$CONFIG_DIR/env" ]; then
            DB_URL=$(grep "^OPS_DATABASE__URL=" "$CONFIG_DIR/env" | cut -d'=' -f2-)
            if [ -n "$DB_URL" ]; then
                log_info "Backing up PostgreSQL database..."
                if pg_dump "$DB_URL" > "$TEMP_DIR/database.sql" 2>/dev/null; then
                    log_success "Database backed up"
                else
                    log_warn "Database backup failed (continuing)"
                fi
            fi
        fi
    else
        log_info "pg_dump not found, skipping database backup"
    fi
fi

# Step 4: Create archive
log_info "Step 4: Creating backup archive"
tar -czf "${BACKUP_NAME}.tar.gz" -C "$TEMP_DIR" . 2>/dev/null
BACKUP_SIZE=$(du -h "${BACKUP_NAME}.tar.gz" | cut -f1)
log_success "Backup archive created: ${BACKUP_NAME}.tar.gz ($BACKUP_SIZE)"

# Step 5: Create checksum
log_info "Step 5: Creating checksum"
if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${BACKUP_NAME}.tar.gz" > "${BACKUP_NAME}.sha256"
    log_success "Checksum created"
elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${BACKUP_NAME}.tar.gz" > "${BACKUP_NAME}.sha256"
    log_success "Checksum created"
fi

# Step 6: Cleanup old backups
log_info "Step 6: Cleaning up old backups"
# Keep last 7 daily backups
find "$BACKUP_DIR" -name "${BINARY_NAME}-backup-*.tar.gz" -mtime +7 -delete 2>/dev/null || true
find "$BACKUP_DIR" -name "${BINARY_NAME}-backup-*.sha256" -mtime +7 -delete 2>/dev/null || true
log_success "Old backups cleaned up"

# Summary
echo ""
log_success "Backup completed successfully!"
echo ""
echo "Backup location: $BACKUP_DIR/${BACKUP_NAME}.tar.gz"
echo "Backup size: $BACKUP_SIZE"
echo "Checksum: $BACKUP_DIR/${BACKUP_NAME}.sha256"
echo ""

if [ "$INSTALL_MODE" = "docker" ]; then
    echo "Backup includes:"
    echo "  ✓ Docker configuration files"
    echo "  ✓ Database dump (if container was running)"
    echo ""
    echo "To restore:"
    echo "  1. Extract archive: tar -xzf ${BACKUP_NAME}.tar.gz -C /tmp/restore"
    echo "  2. Restore configuration: cp -r /tmp/restore/docker/* $DOCKER_DIR/"
    echo "  3. Restore database (if needed):"
    echo "     docker exec -i <postgres_container> psql -U <user> <database> < /tmp/restore/database.sql"
else
    echo "Backup includes:"
    echo "  ✓ Configuration files"
    echo "  ✓ Data directory"
    echo "  ✓ Database dump (if pg_dump available)"
    echo ""
    echo "To restore:"
    echo "  1. Extract archive: tar -xzf ${BACKUP_NAME}.tar.gz -C /tmp/restore"
    echo "  2. Stop service: systemctl stop ${BINARY_NAME}"
    echo "  3. Restore files: cp -r /tmp/restore/* /"
    echo "  4. Restore database (if needed):"
    echo "     psql <database_url> < /tmp/restore/database.sql"
    echo "  5. Start service: systemctl start ${BINARY_NAME}"
fi
echo ""
