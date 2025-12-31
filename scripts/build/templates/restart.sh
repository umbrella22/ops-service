#!/bin/bash
# Restart script for {{BINARY_NAME}}

set -euo pipefail

SERVICE_NAME="{{BINARY_NAME}}"

if [ "$EUID" -ne 0 ]; then
    echo "This script must be run as root or with sudo"
    exit 1
fi

echo "Restarting ${SERVICE_NAME} service..."
systemctl restart "$SERVICE_NAME"

# Wait for service to be active
sleep 2

if systemctl is-active --quiet "$SERVICE_NAME"; then
    echo "✓ Service restarted successfully"
    systemctl status "$SERVICE_NAME" --no-pager
else
    echo "✗ Failed to restart service"
    echo ""
    echo "Check logs with: journalctl -u ${SERVICE_NAME} -n 50"
    exit 1
fi
