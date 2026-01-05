# MCP Client Examples

Example Rust clients for connecting to Stakpak MCP servers.

## Quick Start

```bash
# 1. Generate certificates
cargo run -- mcp setup

# 2. Start server
cargo run -- mcp start --port 8420

# 3. Run client
cd examples/mcp-clients/rust-client
cargo run --example mtls_client
```

## Examples

- **`basic_client.rs`** - Simple connection (testing only, no TLS)
- **`mtls_client.rs`** - Secure mTLS connection (recommended)
- **`tool_calling.rs`** - Call MCP tools with LLM integration (Gemini)
- **`proxy_client.rs`** - Connect via MCP proxy


## Proxy Client Example

The proxy client connects to `stakpak mcp proxy`, which aggregates tools from multiple upstream MCP servers.

### Setup

1. **Create proxy config** at `~/.stakpak/mcp.toml`:
   ```toml
   [mcpServers.filesystem]
   command = "npx"
   args = ["-y", "@modelcontextprotocol/server-filesystem", "/etc"]

   # Connect to Stakpak MCP server (requires mTLS certs at ~/.stakpak/certs/)
   [mcpServers.stakpak]
   url = "https://127.0.0.1:8420/mcp"
   ```

2. **Start Stakpak MCP server** (in a separate terminal):
   ```bash
   cargo run -- mcp start --port 8420
   ```

3. **Run the proxy client**:
   ```bash
   cargo run --example proxy_client
   ```

### How It Works

- The proxy spawns configured MCP servers as child processes (stdio) or connects via HTTP/HTTPS
- Tools are prefixed with server name: `filesystem__read_file`, `stakpak__generate_password`
- **mTLS auto-loading**: HTTPS connections automatically load certificates from `~/.stakpak/certs/`

## Direct Client Usage

```rust
use stakpak_mcp_rust_client::StakpakMCPClient;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let certs_dir = PathBuf::from(std::env::var("HOME")?)
        .join(".stakpak").join("certs");

    let client = StakpakMCPClient::new_with_mtls(
        "https://127.0.0.1:8420",
        &certs_dir.join("ca.pem"),
        &certs_dir.join("client-cert.pem"),
        &certs_dir.join("client-key.pem"),
    ).await?;

    let tools = client.list_tools().await?;
    println!("Available tools: {:?}", tools);

    Ok(())
}
```

## Troubleshooting

**Connection refused?** Ensure server is running:
```bash
cargo run -- mcp start --port 8420
```

**Certificate errors?** Regenerate certificates:
```bash
cargo run -- mcp setup --force
```

## Resources

- [MCP Specification](https://modelcontextprotocol.io/)
- [Stakpak GitHub](https://github.com/stakpak/stakpak)

