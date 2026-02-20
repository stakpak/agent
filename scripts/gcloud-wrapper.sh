#!/bin/bash
# gcloud CLI wrapper - installs on first use
# Pinned version: 555.0.0 (2026-02-06)
set -e

GCLOUD_VERSION="555.0.0"
GCLOUD_DIR="/opt/google-cloud-sdk"
GCLOUD_BIN="${GCLOUD_DIR}/bin/gcloud"

if [[ ! -x "$GCLOUD_BIN" ]]; then
    echo "📦 Installing Google Cloud CLI v${GCLOUD_VERSION} (first-time setup)..." >&2
    
    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64) GCLOUD_ARCH="x86_64" ;;
        aarch64|arm64) GCLOUD_ARCH="arm" ;;
        *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
    esac
    
    cd /tmp
    curl -sSL "https://dl.google.com/dl/cloudsdk/channels/rapid/downloads/google-cloud-cli-${GCLOUD_VERSION}-linux-${GCLOUD_ARCH}.tar.gz" -o gcloud.tar.gz
    sudo tar -xzf gcloud.tar.gz -C /opt
    sudo chown -R agent:agent "$GCLOUD_DIR"
    rm gcloud.tar.gz
    
    "$GCLOUD_DIR/install.sh" --quiet --usage-reporting=false --path-update=false --command-completion=false
    
    # gke-gcloud-auth-plugin is required for kubectl to authenticate with GKE clusters
    "$GCLOUD_BIN" components install gke-gcloud-auth-plugin --quiet
    
    echo "✅ Google Cloud CLI installed successfully" >&2
fi

exec "$GCLOUD_BIN" "$@"
