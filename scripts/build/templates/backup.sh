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
CONFIG_DIR="/etc/${BINARY_NAME}"
DATA_DIR="/var/lib/${BINARY_NAME}"
BACKUP_DIR="/var/backups/${BINARY_NAME}"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
BACKUP_NAME="${BINARY_NAME}-backup-${TIMESTAMP}"

if [ "$EUID" -ne 0 ]; then
    log_error "This script must be run as root or with sudo"
    exit 1
fi

log_info "Creating backup for ${BINARY_NAME}"
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
    cp -r "$CONFIG_DIR" "$TEMP_DIR/config"
    log_success "Configuration backed up"
else
    log_warn "No configuration directory found"
fi

# Step 2: Backup data directory
log_info "Step 2: Backing up data directory"
if [ -d "$DATA_DIR" ]; then
    cp -r "$DATA_DIR" "$TEMP_DIR/data"
    log_success "Data directory backed up"
else
    log_warn "No data directory found"
fi

# Step 3: Backup database (if PostgreSQL is available)
log_info "Step 3: Checking for database backup"
if command -v pg_dump >/dev/null 2>&1; then
    # Try to extract database URL from config
    if [ -f "$CONFIG_DIR/env" ]; then
        DB_URL=$(grep "^OPS_DATABASE_URL=" "$CONFIG_DIR/env" | cut -d'=' -f2-)
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
echo "To restore:"
echo "  tar -xzf ${BACKUP_NAME}.tar.gz -C /tmp/restore"
echo "  # Then restore files from /tmp/restore to appropriate locations"
