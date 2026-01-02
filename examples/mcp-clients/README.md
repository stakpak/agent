# MCP Client Examples

Example Rust client for connecting to the Stakpak MCP server.

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

## Usage

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

**Connection refused?** Ensure server is running on the correct port:
```bash
cargo run -- mcp start --port 8420
```

**Certificate errors?** Regenerate certificates:
```bash
cargo run -- mcp setup --force
```

## Resources

- **MCP Specification**: https://modelcontextprotocol.io/
- **Stakpak**: https://github.com/stakpak/stakpak
