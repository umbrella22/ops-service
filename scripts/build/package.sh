#!/bin/bash
# Package script for creating distribution packages

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

# Parse arguments
PLATFORM=${1:-x86_64}

validate_platform "$PLATFORM"

print_section "Creating Package for Platform: $PLATFORM"

# Build paths
BUILD_DIR="$PROJECT_ROOT/build/linux-$PLATFORM"
TARGET_TRIPLE=$(get_target_triple "$PLATFORM")
SOURCE_BINARY="$PROJECT_ROOT/target/$TARGET_TRIPLE/release/$BINARY_NAME"

# Step 1: Build binary
print_section "Step 1: Building Binary"
BINARY_PATH=$("$SCRIPT_DIR/build.sh" "$PLATFORM")

# Step 2: Create directory structure
print_section "Step 2: Creating Directory Structure"

# Remove old build directory
if [ -d "$BUILD_DIR" ]; then
    log_info "Removing old build directory: $BUILD_DIR"
    rm -rf "$BUILD_DIR"
fi

# Create directories
create_dir "$BUILD_DIR/bin"
create_dir "$BUILD_DIR/migrations"
create_dir "$BUILD_DIR/config"
create_dir "$BUILD_DIR/docker"
create_dir "$BUILD_DIR/nginx"
create_dir "$BUILD_DIR/scripts"
create_dir "$BUILD_DIR/systemd"
create_dir "$BUILD_DIR/docs"
create_dir "$BUILD_DIR/security"

# Step 3: Copy binary
print_section "Step 3: Copying Binary"
copy_file "$BINARY_PATH" "$BUILD_DIR/bin/$BINARY_NAME"
make_executable "$BUILD_DIR/bin/$BINARY_NAME"

# Step 4: Copy migrations
print_section "Step 4: Copying Migrations"
if [ -d "$PROJECT_ROOT/migrations" ]; then
    cp -r "$PROJECT_ROOT/migrations/"* "$BUILD_DIR/migrations/"
    log_success "Copied $(ls "$BUILD_DIR/migrations" | wc -l) migration file(s)"
else
    log_warn "No migrations directory found"
fi

# Step 5: Copy configuration template
print_section "Step 5: Copying Configuration"
if [ -f "$PROJECT_ROOT/.env.example" ]; then
    copy_file "$PROJECT_ROOT/.env.example" "$BUILD_DIR/config/.env.example"
else
    log_warn "No .env.example found"
fi

# Step 6: Copy Docker files
print_section "Step 6: Copying Docker Files"

# 生成基于二进制的 Dockerfile
if [ -f "$SCRIPT_DIR/templates/Dockerfile.package" ]; then
    substitute_vars \
        "$SCRIPT_DIR/templates/Dockerfile.package" \
        "$BUILD_DIR/docker/Dockerfile" \
        "BINARY_NAME=$BINARY_NAME" \
        "VERSION=$VERSION" \
        "PLATFORM=$PLATFORM"
    log_success "Generated Dockerfile for binary package"
else
    log_warn "Dockerfile template not found"
fi

# 复制 docker-compose.yml（如果存在）
if [ -f "$PROJECT_ROOT/docker-compose.yml" ]; then
    copy_file "$PROJECT_ROOT/docker-compose.yml" "$BUILD_DIR/docker/"
fi

# 复制 Docker 构建脚本
if [ -f "$SCRIPT_DIR/templates/docker-build.sh" ]; then
    substitute_vars \
        "$SCRIPT_DIR/templates/docker-build.sh" \
        "$BUILD_DIR/docker/build.sh" \
        "BINARY_NAME=$BINARY_NAME" \
        "VERSION=$VERSION" \
        "PLATFORM=$PLATFORM"
    chmod +x "$BUILD_DIR/docker/build.sh"
    log_success "Generated Docker build script"
fi

# 复制 Docker README
if [ -f "$SCRIPT_DIR/templates/docs/DOCKER.md" ]; then
    substitute_vars \
        "$SCRIPT_DIR/templates/docs/DOCKER.md" \
        "$BUILD_DIR/docker/README.md" \
        "BINARY_NAME=$BINARY_NAME" \
        "VERSION=$VERSION" \
        "PLATFORM=$PLATFORM"
    log_success "Generated Docker README"
fi

# 复制安全配置模板
if [ -f "$SCRIPT_DIR/templates/docker-compose.secure.yml" ]; then
    substitute_vars \
        "$SCRIPT_DIR/templates/docker-compose.secure.yml" \
        "$BUILD_DIR/docker/docker-compose.secure.yml" \
        "BINARY_NAME=$BINARY_NAME" \
        "VERSION=$VERSION" \
        "PLATFORM=$PLATFORM"
    log_success "Generated secure docker-compose template"
fi

# 复制 AppArmor 配置模板
if [ -f "$SCRIPT_DIR/templates/AppArmor.profile" ]; then
    substitute_vars \
        "$SCRIPT_DIR/templates/AppArmor.profile" \
        "$BUILD_DIR/security/AppArmor.profile" \
        "BINARY_NAME=$BINARY_NAME" \
        "VERSION=$VERSION" \
        "PLATFORM=$PLATFORM"
    log_success "Generated AppArmor security profile"
fi

# Step 7: Copy Nginx configuration
print_section "Step 7: Copying Nginx Configuration"
if [ -d "$PROJECT_ROOT/nginx" ]; then
    cp -r "$PROJECT_ROOT/nginx/"* "$BUILD_DIR/nginx/"
    log_success "Copied Nginx configuration files"
else
    log_warn "No nginx directory found"
fi

# Step 8: Generate management scripts
print_section "Step 8: Generating Management Scripts"
if [ -f "$SCRIPT_DIR/generate_scripts.sh" ]; then
    "$SCRIPT_DIR/generate_scripts.sh" "$BUILD_DIR" "$VERSION" "$PLATFORM"
else
    log_warn "Script generator not found, skipping"
fi

# 复制 scripts README
if [ -f "$SCRIPT_DIR/templates/scripts/README.md" ]; then
    substitute_vars \
        "$SCRIPT_DIR/templates/scripts/README.md" \
        "$BUILD_DIR/scripts/README.md" \
        "BINARY_NAME=$BINARY_NAME" \
        "VERSION=$VERSION" \
        "PLATFORM=$PLATFORM"
    log_success "Generated scripts README"
fi

# Step 9: Generate systemd service file
print_section "Step 9: Generating Systemd Service"
if [ -f "$SCRIPT_DIR/templates/systemd.service" ]; then
    substitute_vars \
        "$SCRIPT_DIR/templates/systemd.service" \
        "$BUILD_DIR/systemd/$BINARY_NAME.service" \
        "BINARY_NAME=$BINARY_NAME" \
        "VERSION=$VERSION"
    log_success "Created systemd service file"
else
    log_warn "Systemd template not found"
fi

# Step 10: Generate documentation
print_section "Step 10: Generating Documentation"
if [ -d "$SCRIPT_DIR/templates/docs" ]; then
    for doc in "$SCRIPT_DIR/templates/docs/"*.md; do
        if [ -f "$doc" ]; then
            doc_name=$(basename "$doc")
            substitute_vars \
                "$doc" \
                "$BUILD_DIR/docs/$doc_name" \
                "BINARY_NAME=$BINARY_NAME" \
                "VERSION=$VERSION" \
                "PLATFORM=$PLATFORM"
            log_info "Generated documentation: $doc_name"
        fi
    done
else
    log_warn "Documentation templates not found"
fi

# Step 11: Create VERSION file
print_section "Step 11: Creating Metadata Files"
echo "$VERSION" > "$BUILD_DIR/VERSION"
log_info "Created VERSION file"

# Step 12: Create BUILD_INFO.txt
cat > "$BUILD_DIR/BUILD_INFO.txt" <<EOF
Build Information
================
Project:         $BINARY_NAME
Version:         $VERSION
Platform:        $PLATFORM
Target Triple:   $TARGET_TRIPLE
Build Time:      $TIMESTAMP
Rust Version:    $(get_rust_version)
Git Branch:      $(get_git_branch)
Git Commit:      $(get_git_commit)

Build Host:      $(hostname)
Build User:      $(whoami)
EOF
log_info "Created BUILD_INFO.txt"

# Step 13: Calculate checksums
print_section "Step 13: Calculating Checksums"
CHECKSUM_FILE="$BUILD_DIR/CHECKSUM"
echo "# SHA256 Checksums" > "$CHECKSUM_FILE"
echo "# Generated: $TIMESTAMP" >> "$CHECKSUM_FILE"
echo "" >> "$CHECKSUM_FILE"

# Find all files and calculate checksums
while IFS= read -r -d '' file; do
    if [ "$file" != "$CHECKSUM_FILE" ]; then
        checksum=$(calculate_checksum "$file")
        relative_path="${file#$BUILD_DIR/}"
        echo "$checksum  $relative_path" >> "$CHECKSUM_FILE"
    fi
done < <(find "$BUILD_DIR" -type f -print0)

log_success "Created CHECKSUM file"

# Summary
print_section "Package Creation Complete"
log_success "Package created at: $BUILD_DIR"
log_info "Version: $VERSION"
log_info "Platform: $PLATFORM"
log_info "Binary: $BUILD_DIR/bin/$BINARY_NAME"

# Display package contents
echo ""
log_info "Package structure:"
tree -L 2 "$BUILD_DIR" 2>/dev/null || find "$BUILD_DIR" -type d | head -20

# Calculate total size
TOTAL_SIZE=$(du -sh "$BUILD_DIR" | awk '{print $1}')
log_success "Total package size: $TOTAL_SIZE"
