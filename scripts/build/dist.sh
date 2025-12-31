#!/bin/bash
# Create distribution archives

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

if [ $# -lt 1 ]; then
    log_error "Usage: $0 <platform>"
    exit 1
fi

PLATFORM=$1
validate_platform "$PLATFORM"

print_section "Creating Distribution Archive: $PLATFORM"

BUILD_DIR="$PROJECT_ROOT/build/linux-$PLATFORM"
DIST_DIR="$PROJECT_ROOT/build/dist"
ARCHIVE_NAME="${BINARY_NAME}-${VERSION}-linux-${PLATFORM}.tar.gz"

# Create dist directory
create_dir "$DIST_DIR"

# Check if build directory exists
if [ ! -d "$BUILD_DIR" ]; then
    log_error "Build directory not found: $BUILD_DIR"
    log_error "Run 'make package-$PLATFORM' first"
    exit 1
fi

# Create archive
log_info "Creating archive: $ARCHIVE_NAME"
cd "$BUILD_DIR"
tar -czf "$DIST_DIR/$ARCHIVE_NAME" .
ARCHIVE_SIZE=$(du -h "$DIST_DIR/$ARCHIVE_NAME" | cut -f1)
log_success "Archive created: $DIST_DIR/$ARCHIVE_NAME ($ARCHIVE_SIZE)"

# Create checksum
log_info "Creating checksum"
cd "$DIST_DIR"
CHECKSUM_FILE="${ARCHIVE_NAME}.sha256"
calculate_checksum "$ARCHIVE_NAME" > "$CHECKSUM_FILE"
log_success "Checksum created: $CHECKSUM_FILE"

# Display checksum
echo ""
log_info "SHA256 Checksum:"
cat "$CHECKSUM_FILE"

# Create latest version marker
echo "$VERSION" > "$DIST_DIR/${BINARY_NAME}-latest-version.txt"
log_info "Version marker created"

# List all distributions
echo ""
log_info "Available distributions:"
ls -lh "$DIST_DIR/" 2>/dev/null || log_warn "No distributions found yet"

log_success "Distribution archive created successfully!"
