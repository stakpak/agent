# mTLS Setup Guide

This guide explains how to set up mutual TLS (mTLS) authentication for the Stakpak MCP server.

## Quick Setup

```bash
# 1. Generate certificates (one-time setup)
cargo run -- mcp setup

# 2. Start server with mTLS enabled
cargo run -- mcp start --port 8420
```

This creates certificates in `~/.stakpak/certs/`:
- `ca.pem`
- `server-cert.pem`
- `server-key.pem`
- `client-cert.pem`
- `client-key.pem`

## Using Certificates in Clients

### Environment Variables

export them

```bash
export MCP_SERVER_URL="https://127.0.0.1:8420"
export CERTS_DIR="$HOME/.stakpak/certs"
# for the LLM integration example
export GEMINI_API_KEY=your_gemini_api_key
```

or copy `.env.example` to `.env` and modify it

### Certificate Files Needed

All clients need these 3 files:
- `ca.pem`
- `client-cert.pem`
- `client-key.pem`

## Server Configuration

### Default Setup
```bash
# Uses ~/.stakpak/certs by default
cargo run -- mcp setup
cargo run -- mcp start --port 8420
```

### Custom Certificate Directory
```bash
# Generate in custom location
cargo run -- mcp setup --out-dir /path/to/certs

# Start server with custom certs
cargo run -- mcp start --config-dir /path/to/certs --port 8420
```

### Regenerate Certificates
```bash
# Force regenerate (overwrites existing certificates)
cargo run -- mcp setup --force
```

## Testing Without mTLS

```bash
# Start server without mTLS
cargo run -- mcp start --disable-mcp-mtls

# Use HTTP instead of HTTPS
export MCP_SERVER_PORT=8420
```

## Port Configuration
The server binds to `0.0.0.0:8420` (all interfaces), but clients should connect to `127.0.0.1:8420` (localhost):

Use `--port 8420` to ensure server and client use the same port.
