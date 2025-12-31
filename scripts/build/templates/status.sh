#!/bin/bash
# Status check script for {{BINARY_NAME}}

set -euo pipefail

SERVICE_NAME="{{BINARY_NAME}}"
BINARY_NAME="{{BINARY_NAME}}"
CONFIG_DIR="/etc/${BINARY_NAME}"

echo "======================================"
echo "Service Status: ${SERVICE_NAME}"
echo "======================================"
echo ""

# Systemd status
if systemctl is-active --quiet "$SERVICE_NAME"; then
    echo "Status: RUNNING ✓"
else
    echo "Status: STOPPED ✗"
fi
echo ""

# Show systemd status
systemctl status "$SERVICE_NAME" --no-pager -l || true

echo ""
echo "======================================"
echo "Recent Logs (last 20 lines)"
echo "======================================"
journalctl -u "$SERVICE_NAME" -n 20 --no-pager || true

echo ""
echo "======================================"
echo "Health Check"
echo "======================================"
HEALTH_URL="${OPS_HEALTH_URL:-http://localhost:3000/health}"
if command -v curl >/dev/null 2>&1; then
    if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
        echo "Health endpoint: OK ✓"
    else
        echo "Health endpoint: FAILED ✗"
    fi
else
    echo "curl not found, skipping health check"
fi

echo ""
echo "======================================"
echo "Configuration"
echo "======================================"
if [ -f "$CONFIG_DIR/env" ]; then
    echo "Config file: $CONFIG_DIR/env"
    echo "Last modified: $(stat -c %y "$CONFIG_DIR/env" 2>/dev/null || stat -f %Sm "$CONFIG_DIR/env" 2>/dev/null)"
else
    echo "Config file: NOT FOUND"
fi
