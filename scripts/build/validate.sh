#!/bin/bash
# Validate package contents

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

print_section "Validating Build Package"

# Find build directory (prefer x86_64 for validation)
BUILD_DIR=""
if [ -d "$PROJECT_ROOT/build/linux-x86_64" ]; then
    BUILD_DIR="$PROJECT_ROOT/build/linux-x86_64"
elif [ -d "$PROJECT_ROOT/build/linux-arm64" ]; then
    BUILD_DIR="$PROJECT_ROOT/build/linux-arm64"
else
    log_error "No build directory found"
    exit 1
fi

log_info "Validating: $BUILD_DIR"
echo ""

VALIDATION_PASSED=true

# Function to check file
check_file() {
    local file=$1
    local perms=$2
    local description=$3

    if [ ! -e "$file" ]; then
        log_error "Missing: $description ($file)"
        VALIDATION_PASSED=false
        return 1
    fi

    if [ -n "$perms" ]; then
        actual_perms=$(stat -c %a "$file" 2>/dev/null || stat -f %A "$file" 2>/dev/null)
        if [ "$actual_perms" != "$perms" ]; then
            log_warn "Permissions: $description (expected $perms, got $actual_perms)"
        fi
    fi

    log_success "Found: $description"
    return 0
}

# Check if directory exists
check_dir() {
    local dir=$1
    local description=$2

    if [ ! -d "$dir" ]; then
        log_error "Missing: $description ($dir)"
        VALIDATION_PASSED=false
        return 1
    fi

    log_success "Found: $description"
    return 0
}

# Required files
log_info "Checking required files..."
echo ""

check_file "$BUILD_DIR/VERSION" "" "VERSION file"
check_file "$BUILD_DIR/BUILD_INFO.txt" "" "BUILD_INFO.txt"
check_file "$BUILD_DIR/CHECKSUM" "" "CHECKSUM file"

# Binary
echo ""
log_info "Checking binary..."
check_file "$BUILD_DIR/bin/$BINARY_NAME" "755" "Binary executable"
check_file "$BUILD_DIR/bin/ops-runner" "755" "Runner executable"

# Test binary execution
if [ -x "$BUILD_DIR/bin/$BINARY_NAME" ]; then
    log_info "Testing binary execution..."
    if "$BUILD_DIR/bin/$BINARY_NAME" --version >/dev/null 2>&1; then
        VERSION_OUTPUT=$("$BUILD_DIR/bin/$BINARY_NAME" --version 2>&1 || true)
        log_success "Binary executes: $VERSION_OUTPUT"
    else
        log_warn "Binary doesn't support --version (this is okay)"
    fi
fi
if [ -x "$BUILD_DIR/bin/ops-runner" ]; then
    log_info "Testing runner execution..."
    if "$BUILD_DIR/bin/ops-runner" --version >/dev/null 2>&1; then
        VERSION_OUTPUT=$("$BUILD_DIR/bin/ops-runner" --version 2>&1 || true)
        log_success "Runner executes: $VERSION_OUTPUT"
    else
        log_warn "Runner doesn't support --version (this is okay)"
    fi
fi

# Directories
echo ""
log_info "Checking directories..."
check_dir "$BUILD_DIR/migrations" "Migrations directory"
check_dir "$BUILD_DIR/config" "Config directory"
check_dir "$BUILD_DIR/docker" "Docker directory"
check_dir "$BUILD_DIR/nginx" "Nginx directory"
check_dir "$BUILD_DIR/scripts" "Scripts directory"
check_dir "$BUILD_DIR/systemd" "Systemd directory"
check_dir "$BUILD_DIR/docs" "Documentation directory"

# Check migrations
echo ""
log_info "Checking migrations..."
if [ -d "$BUILD_DIR/migrations" ]; then
    MIGRATION_COUNT=$(ls "$BUILD_DIR/migrations"/*.sql 2>/dev/null | wc -l)
    if [ "$MIGRATION_COUNT" -gt 0 ]; then
        log_success "Found $MIGRATION_COUNT migration file(s)"
    else
        log_warn "No migration files found"
    fi
fi

# Check scripts
echo ""
log_info "Checking scripts..."
REQUIRED_SCRIPTS=("install.sh" "start.sh" "stop.sh" "restart.sh" "status.sh" "update.sh" "backup.sh" "uninstall.sh")
for script in "${REQUIRED_SCRIPTS[@]}"; do
    check_file "$BUILD_DIR/scripts/$script" "755" "Script: $script"
done

# Check systemd service
echo ""
log_info "Checking systemd service..."
check_file "$BUILD_DIR/systemd/$BINARY_NAME.service" "644" "Systemd service file"
check_file "$BUILD_DIR/systemd/ops-runner.service" "644" "Runner systemd service file"

# Check Docker files
echo ""
log_info "Checking Docker files..."
check_file "$BUILD_DIR/docker/Dockerfile" "" "Dockerfile"
check_file "$BUILD_DIR/docker/docker-compose.yml" "" "docker-compose.yml"

# Check documentation
echo ""
log_info "Checking documentation..."
REQUIRED_DOCS=("DEPLOY.md" "UPGRADE.md" "TROUBLESHOOTING.md")
for doc in "${REQUIRED_DOCS[@]}"; do
    check_file "$BUILD_DIR/docs/$doc" "" "Documentation: $doc"
done

# Validate checksums
echo ""
log_info "Validating checksums..."
if [ -f "$BUILD_DIR/CHECKSUM" ]; then
    # Check if all files in CHECKSUM actually exist
    MISSING_FILES=0
    while read -r line; do
        # Skip empty lines and comments
        if [[ -z "$line" ]] || [[ "$line" =~ ^# ]]; then
            continue
        fi

        # Extract filename (second field in checksum line)
        file=$(echo "$line" | awk '{print $2}')

        if [ -n "$file" ] && [ ! -f "$BUILD_DIR/$file" ]; then
            log_error "File in CHECKSUM missing: $file"
            MISSING_FILES=$((MISSING_FILES + 1))
        fi
    done < "$BUILD_DIR/CHECKSUM"

    if [ $MISSING_FILES -eq 0 ]; then
        log_success "All files in CHECKSUM exist"
    else
        log_error "$MISSING_FILES file(s) missing from CHECKSUM"
        VALIDATION_PASSED=false
    fi
fi

# Verify VERSION file content
echo ""
log_info "Verifying version..."
if [ -f "$BUILD_DIR/VERSION" ]; then
    PACKAGE_VERSION=$(cat "$BUILD_DIR/VERSION")
    if [ "$PACKAGE_VERSION" = "$VERSION" ]; then
        log_success "Version matches: $PACKAGE_VERSION"
    else
        log_error "Version mismatch (expected $VERSION, got $PACKAGE_VERSION)"
        VALIDATION_PASSED=false
    fi
fi

# Summary
echo ""
print_section "Validation Summary"
if [ "$VALIDATION_PASSED" = true ]; then
    log_success "All validations passed!"
    exit 0
else
    log_error "Some validations failed!"
    exit 1
fi
