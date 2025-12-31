#!/bin/bash
# Generate management scripts from templates

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

if [ $# -lt 2 ]; then
    log_error "Usage: $0 <build_dir> <version> [platform]"
    exit 1
fi

BUILD_DIR="$1"
VERSION="$2"
PLATFORM="${3:-linux}"

SCRIPTS_DIR="$BUILD_DIR/scripts"
TEMPLATES_DIR="$SCRIPT_DIR/templates"

log_info "Generating management scripts for version $VERSION"

# Copy and process each template
for template in "$TEMPLATES_DIR"/*.sh; do
    if [ -f "$template" ]; then
        script_name=$(basename "$template")

        # 跳过 Docker 相关脚本（这些脚本由 docker/ 目录的打包逻辑处理）
        if [[ "$script_name" == docker-* ]]; then
            log_info "Skipping Docker script: $script_name (handled by docker packaging)"
            continue
        fi

        output_script="$SCRIPTS_DIR/$script_name"

        # Substitute variables
        substitute_vars \
            "$template" \
            "$output_script" \
            "BINARY_NAME=$BINARY_NAME" \
            "VERSION=$VERSION" \
            "PLATFORM=$PLATFORM"

        # Make executable
        make_executable "$output_script"
    fi
done

log_success "Generated $(ls "$SCRIPTS_DIR"/*.sh 2>/dev/null | wc -l) script(s)"
