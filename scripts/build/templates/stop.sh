#!/bin/bash
# Stop script for {{BINARY_NAME}}

set -euo pipefail

SERVICE_NAME="{{BINARY_NAME}}"

if [ "$EUID" -ne 0 ]; then
    echo "This script must be run as root or with sudo"
    exit 1
fi

echo "Stopping ${SERVICE_NAME} service..."
systemctl stop "$SERVICE_NAME"

# Wait for service to stop
sleep 2

if systemctl is-active --quiet "$SERVICE_NAME"; then
    echo "✗ Service is still running"
    exit 1
else
    echo "✓ Service stopped successfully"
fi
