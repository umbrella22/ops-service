#!/bin/bash
# Common functions for build scripts

set -euo pipefail

# Colors for output
readonly COLOR_RED='\033[0;31m'
readonly COLOR_GREEN='\033[0;32m'
readonly COLOR_YELLOW='\033[1;33m'
readonly COLOR_BLUE='\033[0;34m'
readonly COLOR_RESET='\033[0m'

# Logging functions
log_info() {
    echo -e "${COLOR_BLUE}[INFO]${COLOR_RESET} $*"
}

log_success() {
    echo -e "${COLOR_GREEN}[✓]${COLOR_RESET} $*"
}

log_warn() {
    echo -e "${COLOR_YELLOW}[WARN]${COLOR_RESET} $*"
}

log_error() {
    echo -e "${COLOR_RED}[ERROR]${COLOR_RESET} $*" >&2
}

# Print section header
print_section() {
    echo ""
    echo -e "${COLOR_BLUE}========== $1 ==========${COLOR_RESET}"
}

# Get version from Cargo.toml
get_version() {
    grep '^version = ' "$PROJECT_ROOT/src/ops-service/Cargo.toml" | head -1 | awk -F'"' '{print $2}'
}

# Get project root directory (workspace root)
get_project_root() {
    cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd
}

# Get binary name from ops-service Cargo.toml
get_binary_name() {
    grep '^name = ' "$PROJECT_ROOT/src/ops-service/Cargo.toml" | head -1 | awk -F'"' '{print $2}'
}

# Get target triple for platform
get_target_triple() {
    local platform=$1
    case "$platform" in
        x86_64)
            echo "x86_64-unknown-linux-gnu"
            ;;
        arm64)
            echo "aarch64-unknown-linux-gnu"
            ;;
        *)
            log_error "Unknown platform: $platform"
            exit 1
            ;;
    esac
}

# Check if command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Check for required tools
check_required_tools() {
    local missing_tools=()

    for tool in "$@"; do
        if ! command_exists "$tool"; then
            missing_tools+=("$tool")
        fi
    done

    if [ ${#missing_tools[@]} -gt 0 ]; then
        log_error "Missing required tools: ${missing_tools[*]}"
        exit 1
    fi
}

# Create directory with parents and log
create_dir() {
    local dir=$1
    mkdir -p "$dir"
    log_info "Created directory: $dir"
}

# Copy file with logging
copy_file() {
    local src=$1
    local dst=$2
    cp "$src" "$dst"
    log_info "Copied: $(basename "$src") -> $dst"
}

# Calculate SHA256 checksum
calculate_checksum() {
    local file=$1
    if command_exists sha256sum; then
        sha256sum "$file" | awk '{print $1}'
    elif command_exists shasum; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        log_error "No SHA256 tool found"
        exit 1
    fi
}

# Verify checksum
verify_checksum() {
    local file=$1
    local expected=$2

    local actual
    actual=$(calculate_checksum "$file")

    if [ "$actual" = "$expected" ]; then
        log_success "Checksum verified: $file"
        return 0
    else
        log_error "Checksum mismatch for $file"
        log_error "Expected: $expected"
        log_error "Actual:   $actual"
        return 1
    fi
}

# Make file executable
make_executable() {
    local file=$1
    chmod 755 "$file"
    log_info "Made executable: $file"
}

# Substitute variables in template
substitute_vars() {
    local template=$1
    local output=$2
    shift 2
    local vars=("$@")

    # Copy template to output
    cp "$template" "$output"

    # Use sed for each variable replacement
    for var in "${vars[@]}"; do
        local var_name="${var%%=*}"
        local var_value="${var#*=}"

        # Escape special characters in the replacement value
        local escaped_value
        escaped_value=$(printf '%s\n' "$var_value" | sed 's/[&/\]/\\&/g')

        # Replace {{VAR_NAME}} with actual value
        sed -i "s|{{$var_name}}|${escaped_value}|g" "$output"
    done
}

# Get current timestamp in UTC
get_timestamp() {
    date -u +"%Y-%m-%dT%H:%M:%SZ"
}

# Get Rust version
get_rust_version() {
    rustc --version | awk '{print $2}'
}

# Get Git commit hash (short)
get_git_commit() {
    git rev-parse --short HEAD 2>/dev/null || echo "unknown"
}

# Get Git branch
get_git_branch() {
    git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown"
}

# Check if cross-compilation toolchain is installed
check_cross_toolchain() {
    local platform=$1

    if [ "$platform" = "arm64" ]; then
        if ! command_exists aarch64-linux-gnu-gcc; then
            log_error "ARM64 cross-compilation toolchain not found"
            log_error "Install with: sudo apt install gcc-aarch64-linux-gnu"
            exit 1
        fi
    fi
}

# Validate platform argument
validate_platform() {
    local platform=$1

    case "$platform" in
        x86_64|arm64)
            return 0
            ;;
        *)
            log_error "Invalid platform: $platform (valid: x86_64, arm64)"
            exit 1
            ;;
    esac
}

# Cleanup on exit
cleanup_on_error() {
    local exit_code=$?
    if [ $exit_code -ne 0 ]; then
        log_error "Build failed with exit code: $exit_code"
    fi
}

trap cleanup_on_error EXIT

# Initialize project root
export PROJECT_ROOT
PROJECT_ROOT=$(get_project_root)

export VERSION
VERSION=$(get_version)

export BINARY_NAME
BINARY_NAME=$(get_binary_name)

export RUNNER_BINARY_NAME
RUNNER_BINARY_NAME="ops-runner"

export TIMESTAMP
TIMESTAMP=$(get_timestamp)
