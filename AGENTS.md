# AGENTS.md — Stakpak CLI

> Agent guidance for working effectively in this codebase.

## Project Overview

Stakpak is a **security-hardened DevOps AI agent** that runs in the terminal. It generates infrastructure code, debugs Kubernetes, configures CI/CD, and automates deployments — without giving the LLM keys to production.

- **Language**: Rust (edition 2024, nightly features enabled)
- **License**: Apache-2.0
- **Repository**: https://github.com/stakpak/agent

## Workspace Structure

```
cli/                          # Main binary crate (`stakpak`)
├── src/
│   ├── main.rs
│   ├── commands/
│   │   ├── agent/run/        # Agent execution engine
│   │   │   ├── mode_interactive.rs   # Interactive TUI agent loop
│   │   │   ├── mode_async.rs         # Async/headless mode
│   │   │   ├── stream.rs             # SSE stream processing
│   │   │   ├── checkpoint.rs         # Session checkpoint/resume
│   │   │   ├── tooling.rs            # Tool execution
│   │   │   └── helpers.rs            # Shared helpers
│   │   ├── acp/              # Agent Client Protocol (Zed integration)
│   │   ├── mcp/              # MCP server/proxy commands
│   │   ├── auth/             # Login/account commands (interactive + non-interactive setup)
│   │   ├── autopilot/        # Autopilot: init, up/down, status, schedule, channel
│   │   └── watch/            # Scheduled task runtime (internal, driven by autopilot)
│   ├── config/               # Configuration management
│   │   ├── file.rs           # ConfigFile with profiles + ensure_readonly()
│   │   ├── profile.rs        # ProfileConfig + readonly_profile()
│   │   └── types.rs          # ProviderType (Remote/Local)
│   └── onboarding/           # Interactive setup wizard + save_config.rs
tui/                          # TUI crate (ratatui-based)
├── src/
│   ├── app/events.rs         # InputEvent / OutputEvent enums
│   └── services/handlers/    # Event handlers (tool, shell, etc.)
libs/
├── ai/                       # LLM provider abstraction (`stakai`)
│   └── src/providers/
│       ├── anthropic/        # Anthropic API (convert, stream, types)
│       ├── openai/           # OpenAI-compatible API
│       └── gemini/           # Google Gemini API
├── api/                      # API client + local processing (`stakpak-api`)
│   └── src/local/
│       ├── context_managers/ # Message history reduction strategies
│       │   ├── task_board_context_manager.rs   # Preserves individual messages
│       │   ├── simple_context_manager.rs       # Flattens history to text
│       │   └── file_scratchpad_context_manager.rs
│       └── hooks/            # Context hooks (scratchpad, task board)
├── shared/                   # Shared types (`stakpak-shared`)
│   └── src/models/
│       ├── llm.rs            # LLMMessage, LLMMessageContent, provider configs
│       ├── stakai_adapter.rs # ChatMessage → StakAI Message conversion
│       └── integrations/
│           ├── openai.rs     # ChatMessage, ToolCall, Role types
│           └── mcp.rs        # MCP tool call result handling
└── mcp/                      # MCP client/server/proxy crates
    ├── client/
    ├── server/
    └── proxy/
```

## Autopilot Architecture

The autopilot system (`stakpak autopilot` / `stakpak up`) is the self-driving infrastructure mode. It runs as a system service (launchd on macOS, systemd on Linux) and manages two runtimes:

### Config: `~/.stakpak/autopilot.toml`

Single config file for everything — schedules, channels, and runtime settings:

```toml
[runtime]
bind = "127.0.0.1:4096"

[[schedules]]
name = "health-check"
cron = "*/5 * * * *"
prompt = "Check system health"

[channels.slack]
bot_token = "xoxb-..."
app_token = "xapp-..."
```

### CLI Commands

```
stakpak up                              # Start autopilot (auto-inits if needed)
stakpak down                            # Stop autopilot
stakpak autopilot init                  # Explicit setup wizard
stakpak autopilot status                # Health, uptime, schedules, channels
stakpak autopilot logs                  # Stream logs
stakpak autopilot schedule list         # List schedules
stakpak autopilot schedule add <name> --cron '...' --prompt '...'
stakpak autopilot schedule remove <name>
stakpak autopilot schedule enable|disable <name>
stakpak autopilot schedule trigger <name>   # Manual fire
stakpak autopilot schedule history <name>
stakpak autopilot channel list          # List channels
stakpak autopilot channel add <type> --token|--bot-token|--app-token
stakpak autopilot channel remove <type>
stakpak autopilot channel test          # Test connectivity
```

### Key Files

| File | Purpose |
|------|---------|
| `cli/src/commands/autopilot.rs` | All autopilot commands, config types, schedule/channel CRUD |
| `cli/src/commands/watch/` | Schedule runtime (cron engine, trigger execution, history) |
| `libs/gateway/` | Channel runtime (Slack/Telegram/Discord message handling) |
| `libs/gateway/src/config.rs` | `GatewayConfig` — channel config load/save |

### Non-Interactive Setup (CI/scripts)

```bash
stakpak auth login --api-key $KEY
stakpak autopilot schedule add health --cron '0 */6 * * *' --prompt 'Check health'
stakpak autopilot channel add slack --bot-token $SLACK_BOT --app-token $SLACK_APP
stakpak up
```

## Architecture & Data Flow

### Message Conversion Pipeline

Messages flow through several transformation layers before reaching the LLM API:

```
User input / Tool results
    ↓
Vec<ChatMessage>                    # OpenAI-shaped messages (cli/mode_interactive.rs)
    ↓  sanitize_tool_results()      # Dedup + remove orphans (before context manager)
    ↓
ContextManager::reduce_context()   # History reduction (libs/api/context_managers/)
    ↓  merge_consecutive_same_role()  # Merge tool messages
    ↓  dedup_tool_results()           # Deduplicate within merged messages
    ↓  reduce_context_with_budget()   # Budget-aware trimming (if over threshold)
    ↓
Vec<LLMMessage>                    # Provider-neutral messages
    ↓
to_stakai_message()                # libs/shared/stakai_adapter.rs
    ↓
Vec<StakAI Message>                # Internal API format
    ↓
build_messages_with_caching()      # libs/ai/providers/anthropic/convert.rs
    ↓
Vec<AnthropicMessage>              # Anthropic API format → HTTP request
```

### Key Types

| Type | Location | Purpose |
|------|----------|---------|
| `ChatMessage` | `libs/shared/models/integrations/openai.rs` | OpenAI-shaped message (role, content, tool_calls, tool_call_id) |
| `LLMMessage` | `libs/shared/models/llm.rs` | Provider-neutral message with typed content parts |
| `LLMMessageContent` | `libs/shared/models/llm.rs` | Either `String` or `List(Vec<LLMMessageTypedContent>)` |
| `LLMMessageTypedContent` | `libs/shared/models/llm.rs` | `Text`, `ToolCall`, `ToolResult`, `Image`, `Document` |
| `AnthropicMessage` | `libs/ai/providers/anthropic/types.rs` | Anthropic API message format |

### Interactive Mode Event Loop

`mode_interactive.rs` runs the main agent loop:

1. **Receive events** from TUI via `output_rx` (`OutputEvent` enum)
2. **Process events**: `UserMessage`, `AcceptTool`, `RejectTool`, `SendToolResult`, etc.
3. **Build message history**: append to `messages: Vec<ChatMessage>`
4. **Sanitize**: `sanitize_tool_results()` before each API call
5. **Send to LLM**: via `client.chat_completion_stream()`
6. **Stream response**: parse SSE events, extract tool calls
7. **Execute tools**: pop from `tools_queue`, send to TUI for approval

### Tool Call Flow

```
AI returns tool_calls [A, B, C]
    ↓
tools_queue = [B, C], send A to TUI
    ↓
TUI: AcceptTool(A) or RejectTool(A)
    ↓  (if accepted)
run_tool_call() → tokio::select! { result OR cancel_signal }
    ↓
Push tool_result(A) to messages
    ↓
Pop B from queue, send to TUI
    ↓  ... repeat ...
All tools done → fall through to API call
```

**Cancel/Retry flow**: When a tool is cancelled (retry/shell mode), the `AcceptTool` handler does NOT push a result if the queue is empty (the shell/retry flow will send `SendToolResult` later). If the queue is non-empty, it pushes a `TOOL_CALL_CANCELLED` placeholder to keep the message chain valid.

## Coding Conventions

### Error Handling

- **`unwrap()` and `expect()` are denied** via `clippy.toml` workspace lints (allowed in tests)
- Use `anyhow::Result` with `?` operator and `.context()` for application code
- Use `thiserror` for library error types
- Use `match` or `if let` for `Option` types

### Style

- **Rust edition 2024** with nightly features (`let chains` in `if let`)
- Run `cargo fmt` before committing
- Run `cargo clippy --all-targets` — warnings should be zero
- Prefer `LLMMessage::from` over `|msg| LLMMessage::from(msg)` (clippy: redundant closure)
- Collapse nested `if` + `if let` into combined conditions where readable
- Use `std::mem::take` for efficient ownership transfer in place

### String Slicing & UTF-8 Char Boundaries

Rust strings are UTF-8. Characters can be 1–4 bytes, so **never slice with a raw byte index** (`&s[..80]`, `&s[..n-3]`) — it panics if the index lands mid-character. Safe approaches:

```rust
// ✅ Truncate by character count
let truncated: String = s.chars().take(80).collect();

// ✅ Validate boundary before slicing (when you need byte-position slicing)
let mut end = max_bytes;
while end > 0 && !s.is_char_boundary(end) { end -= 1; }
let truncated = &s[..end];
```

Indices from `.find()` / `.rfind()` on the same string are always safe. See `cli/src/commands/watch/commands/run.rs:truncate_string()` for the canonical pattern.

### Testing

- Tests live in `#[cfg(test)] mod tests` at the bottom of each file
- Use `#[tokio::test]` for async tests
- Helpers like `assistant_with_tool_calls()`, `tool_message()` abstract test setup
- Assertion helpers like `assert_no_duplicate_tool_results()` encode invariants

### Naming

- Context managers: `<Strategy>ContextManager` (e.g., `TaskBoardContextManager`)
- Event enums: `InputEvent` (TUI → backend), `OutputEvent` (backend → TUI)
- Tool results: `tool_result(id, content)` helper function
- Functions: `snake_case`, descriptive verbs (`sanitize_tool_results`, `merge_consecutive_same_role`)

## Provider-Specific Constraints

### Anthropic API

- **Strictly alternating roles**: `user` / `assistant` must alternate; consecutive same-role messages are rejected (400)
- **Tool results**: `role=tool` messages are converted to `role=user` with `tool_result` content blocks
- **Each `tool_use` needs exactly one `tool_result`**: duplicates or missing results cause 400 errors
- **`tool_result` must reference a `tool_use` in the immediately preceding assistant message**
- Cache control (`ephemeral` breakpoints) is added in `build_messages_with_caching()` — not upstream

### Defense-in-Depth Strategy

The codebase uses **three layers** to prevent invalid message sequences:

1. **Source prevention** (`mode_interactive.rs`): Don't push cancelled tool_results when retry will send the real one
2. **Pre-API sanitization** (`sanitize_tool_results`): Dedup and remove orphans from `Vec<ChatMessage>` before every API call
3. **Context manager** (`task_board_context_manager.rs`): Merge consecutive same-role messages and dedup tool_results in the `reduce_context()` pipeline

### Context Trimming with Cache Preservation

Long sessions accumulate messages that approach the context window limit. The `TaskBoardContextManager` implements budget-aware trimming:

1. **Lazy trimming**: Only triggers when estimated tokens exceed `context_window × threshold` (default 80%)
2. **Stable prefix**: Trimmed messages are replaced with `[trimmed]` placeholders, preserving message structure (roles, tool_call_ids) for API validity
3. **Cache-friendly**: The trimmed prefix produces identical output across turns, so Anthropic's prompt cache stays valid
4. **Metadata persistence**: Trimming state (`trimmed_up_to_message_index`) is stored in `CheckpointState.metadata` and flows through:
   - `CheckpointState.metadata` → `AgentState.metadata` → Hook updates → `save_checkpoint()` → persisted

Key files:
- `libs/api/src/local/context_managers/task_board_context_manager.rs` — `reduce_context_with_budget()`, `estimate_tokens()`, `trim_message()`
- `libs/api/src/local/hooks/task_board_context/mod.rs` — Wires budget-aware trimming into the hook lifecycle
- `libs/api/src/storage.rs` — `CheckpointState.metadata` field
- `libs/api/src/models.rs` — `AgentState.metadata` field

## Build & Test

```bash
# Build
cargo build                         # debug
cargo build --release               # release

# Test
cargo test --workspace              # all tests
cargo test --workspace --lib        # library tests only
cargo test --bin stakpak            # binary tests only
cargo test -p stakpak-api           # single crate
cargo test -- test_name             # by name pattern

# Lint
cargo fmt --check
cargo clippy --all-targets

# Quick check (no codegen)
cargo check
```

## Non-Interactive Setup

The `stakpak auth login` command supports non-interactive setup for CI/scripts:

```bash
# Stakpak API (remote provider, default)
stakpak auth login --api-key $STAKPAK_API_KEY

# Local providers (BYOK)
stakpak auth login --provider anthropic --api-key $ANTHROPIC_API_KEY
stakpak auth login --provider openai --api-key $OPENAI_API_KEY
stakpak auth login --provider gemini --api-key $GEMINI_API_KEY
```

This creates:
- `~/.stakpak/config.toml` with `default` + `readonly` profiles
- `~/.stakpak/auth.toml` for local provider credentials

Full non-interactive autopilot setup:

```bash
stakpak auth login --api-key $STAKPAK_API_KEY
stakpak autopilot init --non-interactive --yes
stakpak autopilot schedule add daily-check --cron '0 9 * * *' --prompt 'Run health checks'
stakpak autopilot channel add slack --bot-token $SLACK_BOT --app-token $SLACK_APP
stakpak up
```

Key files:
- `cli/src/commands/auth/login.rs` — `handle_non_interactive_setup()`
- `cli/src/commands/autopilot.rs` — `setup_autopilot()`, `start_autopilot()`, schedule/channel CRUD
- `cli/src/onboarding/save_config.rs` — `save_to_profile()` + `update_readonly()`
- `cli/src/config/profile.rs` — `readonly_profile()` creates sandbox replica of default
