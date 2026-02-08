#!/bin/bash
# Azure CLI wrapper - installs on first use
# Pinned version: 2.83.0 (2026-02-06)
set -e

AZ_VERSION="2.83.0"
AZ_BIN="/opt/azure-cli/bin/az"

if [[ ! -x "$AZ_BIN" ]]; then
    echo "📦 Installing Azure CLI v${AZ_VERSION} (first-time setup)..." >&2
    
    python3 -m venv /opt/azure-cli
    /opt/azure-cli/bin/pip install --upgrade pip
    /opt/azure-cli/bin/pip install azure-cli==${AZ_VERSION}
    
    echo "✅ Azure CLI installed successfully" >&2
fi

exec "$AZ_BIN" "$@"
