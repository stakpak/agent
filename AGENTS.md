# AGENTS.md

This file provides guidance for AI agents working with the Stakpak codebase.

## Project Overview

Stakpak is an open-source AI DevOps Agent that runs in the terminal. It enables developers to generate infrastructure code, debug Kubernetes, configure CI/CD, and automate deployments with security-first design principles.

**Key differentiators:**
- **Secret Substitution** - LLMs work with credentials without seeing actual values
- **Warden Guardrails** - Network-level policies block destructive operations
- **DevOps Playbooks** - Curated library of DevOps knowledge via Rulebooks

## Architecture

### Workspace Structure

```
stakpak/agent (Rust workspace)
├── cli/                    # CLI binary crate (stakpak-cli)
│   ├── src/commands/       # CLI commands (agent, mcp, auth, acp)
│   ├── src/config/         # Configuration handling
│   └── src/onboarding/     # First-run setup flows
├── tui/                    # Terminal UI crate (stakpak-tui)
│   └── src/services/       # TUI services and handlers
├── libs/
│   ├── ai/                 # AI provider abstraction (stakai)
│   │   └── src/providers/  # Anthropic, OpenAI, Gemini implementations
│   ├── api/                # Stakpak API client (stakpak-api)
│   ├── mcp/
│   │   ├── client/         # MCP client (stakpak-mcp-client)
│   │   ├── server/         # MCP server & tools (stakpak-mcp-server)
│   │   └── proxy/          # MCP proxy (stakpak-mcp-proxy)
│   └── shared/             # Shared utilities (stakpak-shared)
│       ├── src/secrets/    # Secret detection (gitleaks-based)
│       ├── src/models/     # Data models
│       └── src/oauth/      # OAuth providers
```

### Key Crates

| Crate | Purpose |
|-------|---------|
| `stakpak-cli` | Main CLI binary, entry point |
| `stakpak-tui` | Ratatui-based terminal UI |
| `stakai` | Multi-provider AI client (Anthropic, OpenAI, Gemini) |
| `stakpak-mcp-server` | MCP tools (local & remote) |
| `stakpak-shared` | Secret management, file ops, task management |

## Development Guidelines

### Build Commands

```bash
# Development build
cargo build

# Release build
cargo build --release

# Build specific crate
cargo build -p stakpak-cli

# Run locally
cargo run -- --help
```

### Testing

```bash
# All tests
cargo test --workspace

# Specific crate
cargo test -p stakpak-shared

# With output
cargo test --workspace -- --nocapture
```

### Code Quality

```bash
# Format check (required for CI)
cargo fmt --check

# Lint (required for CI)
cargo clippy --all-targets

# Full check
cargo check --all-targets
```

### Critical Lints

The workspace enforces strict linting via `clippy.toml`:

```rust
// ❌ DENIED - will fail CI
let value = option.unwrap();
let value = result.expect("msg");

// ✅ REQUIRED - proper error handling
let value = option.ok_or_else(|| anyhow::anyhow!("missing value"))?;
let value = match option {
    Some(v) => v,
    None => return Err(anyhow::anyhow!("missing value")),
};
```

## Code Patterns

### Error Handling

Use `anyhow` for application errors with context:

```rust
use anyhow::{Result, Context};

fn read_config(path: &Path) -> Result<Config> {
    let contents = std::fs::read_to_string(path)
        .context("Failed to read config file")?;
    toml::from_str(&contents)
        .context("Failed to parse config")
}
```

### Async Code

The project uses `tokio` runtime:

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // ...
}

// In tests
#[tokio::test]
async fn test_async_feature() {
    // ...
}
```

### Secret Handling

Never log or expose secrets. Use the secret manager:

```rust
// Redact secrets before displaying
let redacted = secret_manager.redact_and_store_secrets(&content, Some(&path));

// Restore secrets before execution
let actual = secret_manager.restore_secrets_in_string(&redacted);
```

### MCP Tools

Tools are defined in `libs/mcp/server/src/`:
- `local_tools.rs` - File ops, command execution, task management
- `remote_tools.rs` - API-backed tools (docs search, memory, rulebooks)
- `subagent_tools.rs` - Subagent spawning tools

Tool pattern:

```rust
#[tool(description = "Tool description here")]
pub async fn tool_name(
    &self,
    ctx: RequestContext<RoleServer>,
    Parameters(request): Parameters<RequestType>,
) -> Result<CallToolResult, McpError> {
    // Implementation
}
```

## File Conventions

### Configuration Files

- `~/.stakpak/config.toml` - User configuration
- `.stakpak/` - Project-level session data
- `.warden/` - Security guardrail configs

### Important Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Workspace definition, shared dependencies |
| `clippy.toml` | Clippy configuration (denies unwrap/expect) |
| `cliff.toml` | Changelog generation config |
| `release.sh` | Release automation script |

## CI/CD

GitHub Actions workflow (`.github/workflows/ci.yml`):

1. `cargo check --all-targets`
2. `cargo fmt -- --check`
3. `cargo clippy`
4. `cargo build`
5. `cargo test --workspace`

All checks must pass before merge.

## Common Tasks

### Adding a New MCP Tool

1. Define request struct in appropriate tools file
2. Implement tool method with `#[tool]` attribute
3. Add to tool router macro
4. Test locally with `cargo run -- mcp start --tool-mode local`

### Adding a New AI Provider

1. Create provider module in `libs/ai/src/providers/`
2. Implement `Provider` trait from `libs/ai/src/provider/trait_def.rs`
3. Register in provider dispatcher
4. Add configuration support in `libs/shared/src/models/integrations/`

### Modifying TUI

1. Services in `tui/src/services/`
2. Event handlers in `tui/src/services/handlers/`
3. View rendering in `tui/src/view.rs`
4. Uses Ratatui framework

## Security Considerations

- **Never** commit secrets or credentials
- Use secret redaction for all user-facing output
- Validate all external input
- Be cautious with file system operations
- Test security features with `.warden/test-security.sh`

## Resources

- [Documentation](https://stakpak.gitbook.io/docs)
- [Contributing Guide](./CONTRIBUTING.md)
- [Getting Started](./GETTING-STARTED.md)
