#!/bin/bash
# Build script for compiling the binary

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

# Parse arguments
PLATFORM=${1:-x86_64}

validate_platform "$PLATFORM"

# Redirect all logging to stderr to avoid contaminating output
exec 3>&1 1>&2

print_section "Building for Platform: $PLATFORM"

# Get target triple
TARGET_TRIPLE=$(get_target_triple "$PLATFORM")
export TARGET_TRIPLE

log_info "Target triple: $TARGET_TRIPLE"
log_info "Binary name: $BINARY_NAME"
log_info "Version: $VERSION"

# Check cross-compilation toolchain
check_cross_toolchain "$PLATFORM"

# Change to project root
cd "$PROJECT_ROOT"

# Check if Rust target is installed
if ! rustup target list --installed | grep -q "$TARGET_TRIPLE"; then
    log_info "Installing Rust target: $TARGET_TRIPLE"
    rustup target add "$TARGET_TRIPLE"
fi

# Build
log_info "Starting Cargo build..."
log_info "Build flags: --release --target $TARGET_TRIPLE"

if cargo build --release --target "$TARGET_TRIPLE"; then
    log_success "Build completed successfully"
else
    log_error "Build failed"
    exit 1
fi

# Determine binary location
BINARY_PATH="$PROJECT_ROOT/target/$TARGET_TRIPLE/release/$BINARY_NAME"

# Verify binary exists
if [ ! -f "$BINARY_PATH" ]; then
    log_error "Binary not found at: $BINARY_PATH"
    exit 1
fi

# Get binary size
BINARY_SIZE=$(ls -lh "$BINARY_PATH" | awk '{print $5}')
log_success "Binary created: $BINARY_PATH ($BINARY_SIZE)"

# Test binary (basic execution test)
log_info "Testing binary..."

# Detect if we need to use an emulator for cross-compiled binaries
USE_QEMU=""
if [ "$PLATFORM" = "arm64" ]; then
    # Check if we're cross-compiling (not on native ARM64)
    HOST_ARCH=$(uname -m)
    if [ "$HOST_ARCH" != "aarch64" ]; then
        # We're cross-compiling, try to use QEMU user emulator
        if command_exists qemu-aarch64-static; then
            USE_QEMU="qemu-aarch64-static"
            log_info "Using QEMU static emulator for ARM64 binary"
        elif command_exists qemu-aarch64; then
            USE_QEMU="qemu-aarch64"
            log_info "Using QEMU emulator for ARM64 binary"
        else
            log_warn "QEMU not found, skipping binary test for cross-compiled ARM64"
            log_warn "Install with: sudo apt install qemu-user-static"
        fi
    fi
fi

# Only test if we have a way to execute the binary
if [ -n "$USE_QEMU" ]; then
    if $USE_QEMU "$BINARY_PATH" --version >/dev/null 2>&1; then
        VERSION_OUTPUT=$($USE_QEMU "$BINARY_PATH" --version)
        log_success "Binary test passed: $VERSION_OUTPUT"
    else
        log_error "Binary --version flag failed"
        exit 1
    fi
elif [ "$PLATFORM" != "arm64" ] || [ "$(uname -m)" = "aarch64" ]; then
    # Native build or running on ARM64 host - test directly
    if "$BINARY_PATH" --version >/dev/null 2>&1; then
        VERSION_OUTPUT=$("$BINARY_PATH" --version)
        log_success "Binary test passed: $VERSION_OUTPUT"
    else
        log_error "Binary --version flag failed"
        exit 1
    fi
else
    log_warn "Skipping binary execution test (cross-compiled binary without QEMU)"
    log_warn "Binary will be tested on target platform"
fi

# Output only the binary path to stdout (for capture)
exec 1>&3
echo "$BINARY_PATH"
