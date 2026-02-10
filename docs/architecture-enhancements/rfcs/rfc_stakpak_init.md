# Technical Description & RFC: `/init` Command

> **Target Audience**: Junior developers new to the Stakpak codebase
> **Purpose**: Comprehensive guide to understanding and implementing custom slash commands

---

## Table of Contents

1. [Project Overview](#1-project-overview)
2. [Crate Architecture](#2-crate-architecture)
3. [The CLI Binary Deep Dive](#3-the-cli-binary-deep-dive)
4. [The TUI Crate Deep Dive](#4-the-tui-crate-deep-dive)
5. [Slash Command System](#5-slash-command-system)
6. [Step-by-Step: Adding a New Slash Command](#6-step-by-step-adding-a-new-slash-command)
7. [RFC: `/init` Command](#7-rfc-init-command)

---

## 1. Project Overview

### 1.1 What is Stakpak?

Stakpak is a **DevOps AI Agent** — a command-line tool that helps developers generate infrastructure code, debug Kubernetes, configure CI/CD, and automate deployments. It's essentially a TUI (Terminal User Interface) that communicates with LLM APIs (Claude, GPT, etc.).

### 1.2 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              USER'S TERMINAL                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                         TUI (Terminal UI)                            │   │
│   │                                                                      │   │
│   │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐  │   │
│   │  │ Input Box    │  │ Messages     │  │ Side Panel               │  │   │
│   │  │ (textarea)   │  │ (chat)       │  │ (status, billing, etc.)  │  │   │
│   │  └──────────────┘  └──────────────┘  └──────────────────────────┘  │   │
│   │                                                                      │   │
│   │  ┌──────────────────────────────────────────────────────────────┐  │   │
│   │  │ Helper Dropdown (slash commands, file suggestions)           │  │   │
│   │  └──────────────────────────────────────────────────────────────┘  │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                      │                                       │
│                                      ▼                                       │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                         CLI (Orchestration Layer)                    │   │
│   │                                                                      │   │
│   │  • Parses command-line arguments (clap)                             │   │
│   │  • Loads configuration (~/.stakpak/config.toml)                     │   │
│   │  • Initializes LLM client (AgentClient)                             │   │
│   │  • Spawns TUI and backend communication tasks                       │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                      │                                       │
│                    ┌─────────────────┼─────────────────┐                    │
│                    ▼                 ▼                 ▼                    │
│   ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────────┐     │
│   │     stakai       │  │   stakpak-api    │  │  stakpak-mcp-server  │     │
│   │  (LLM providers) │  │ (Stakpak cloud)  │  │  (Tool execution)    │     │
│   └──────────────────┘  └──────────────────┘  └──────────────────────┘     │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 1.3 Repository Structure

```
agent/
├── Cargo.toml              # Workspace root - defines all crates
├── cli/                    # Main binary crate
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs         # Entry point, CLI parsing
│       ├── config/         # Configuration loading
│       ├── commands/       # Subcommands (agent, auth, mcp, etc.)
│       └── utils/          # Helpers (local_context, gitignore, etc.)
├── tui/                    # Terminal UI crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # Library entry point
│       ├── app.rs          # AppState (all TUI state)
│       ├── event_loop.rs   # Main render/update loop
│       ├── view/           # Rendering components
│       └── services/       # Business logic & handlers
│           ├── commands.rs # SLASH COMMAND REGISTRY ⭐
│           └── handlers/   # Input, shell, tool handlers
├── libs/                   # Shared libraries
│   ├── ai/                 # LLM provider abstractions (stakai)
│   ├── api/                # Stakpak cloud API client
│   ├── shared/             # Common types & utilities
│   └── mcp/                # MCP (Model Context Protocol)
│       ├── client/         # MCP client
│       ├── server/         # MCP server (tool execution)
│       └── proxy/          # mTLS proxy
└── docs/                   # Documentation
```

---

## 2. Crate Architecture

### 2.1 Workspace Layout (Cargo.toml)

The root `Cargo.toml` defines a **Rust workspace**:

```toml
[workspace]
resolver = "2"
members = [
    "cli",              # Main binary
    "tui",              # Terminal UI library
    "libs/shared",      # Shared types
    "libs/api",         # Stakpak API client
    "libs/ai",          # LLM provider abstraction
    "libs/mcp/client",  # MCP client
    "libs/mcp/server",  # MCP server
    "libs/mcp/proxy",   # mTLS proxy
]
default-members = ["cli"]
```

**Key insight**: When you run `cargo run`, it builds the **default member** (`cli`), which produces the `stakpak` binary.

### 2.2 Crate Dependencies

```
                    ┌──────────────────┐
                    │      cli         │  (binary)
                    │   stakpak        │
                    └────────┬─────────┘
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
         ▼                   ▼                   ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│  stakpak-tui    │  │  stakpak-api    │  │     stakai      │
│  (TUI library)  │  │  (API client)   │  │  (LLM client)   │
└────────┬────────┘  └────────┬────────┘  └────────┬────────┘
         │                    │                    │
         └────────────────────┼────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │ stakpak-shared  │
                    │ (common types)  │
                    └─────────────────┘
```

**Important**: The `cli` crate depends on `tui`, **NOT** the other way around. This means:
- ✅ `cli` can use types from `tui`
- ❌ `tui` cannot import anything from `cli`

---

## 3. The CLI Binary Deep Dive

### 3.1 Entry Point: `cli/src/main.rs`

This is the main entry point when you run `stakpak`. Let's break it down:

#### 3.1.1 The Cli Struct (Argument Parsing)

```rust
// cli/src/main.rs, lines 37-142

#[derive(Parser, PartialEq)]
#[command(name = "stakpak")]
#[command(about = "Stakpak CLI tool", long_about = None)]
struct Cli {
    /// Run the agent in async mode (multiple steps until completion)
    #[arg(short = 'a', long = "async", default_value_t = false)]
    r#async: bool,

    /// Configuration profile to use (can also be set with STAKPAK_PROFILE env var)
    #[arg(long = "profile")]
    profile: Option<String>,

    /// Prompt to run the agent
    prompt: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
    
    // ... many more fields
}
```

**How clap works**:
1. `#[derive(Parser)]` generates argument parsing code at compile time
2. Each `#[arg(...)]` attribute defines a CLI flag
3. `#[command(subcommand)]` allows subcommands like `stakpak config`, `stakpak auth`

#### 3.1.2 Argument Parsing Flow

When you run `cargo run -- --profile team`:

```
              cargo run           --         --profile team
                 │                │                │
                 ▼                ▼                ▼
         ┌───────────┐     ┌───────────┐    ┌───────────────┐
         │   Cargo   │     │ Separator │    │   Stakpak     │
         │  consumes │     │  (stop!)  │    │   receives    │
         │   "run"   │     └───────────┘    │  these args   │
         └───────────┘                      └───────────────┘
                                                    │
                                                    ▼
                                           ┌───────────────┐
                                           │  Cli::parse() │
                                           │  with clap    │
                                           └───────────────┘
                                                    │
                                                    ▼
                                           profile = Some("team")
```

**The `--` separator**: This is a POSIX standard. It tells Cargo: "Stop processing arguments. Pass everything after this to the subprocess."

#### 3.1.3 Profile Resolution

```rust
// cli/src/main.rs, lines 191-195

// Priority: CLI arg > Environment variable > "default"
let profile_name = cli
    .profile
    .or_else(|| std::env::var("STAKPAK_PROFILE").ok())
    .unwrap_or_else(|| "default".to_string());
```

Profiles are stored in `~/.stakpak/config.toml`:

```toml
# ~/.stakpak/config.toml

[default]
api_key = "sk-..."
api_endpoint = "https://api.stakpak.dev"

[team]
api_key = "sk-team-..."
api_endpoint = "https://api.team.stakpak.io"
```

#### 3.1.4 The Main Execution Flow

```rust
// cli/src/main.rs, lines 231-515

match cli.command {
    Some(command) => {
        // Subcommand mode: `stakpak config`, `stakpak auth`, etc.
        command.run(config).await?;
    }
    None => {
        // Interactive mode: Launch the TUI
        
        // 1. Analyze local context (detects Terraform, Docker, etc.)
        let local_context = analyze_local_context(&config).await.ok();
        
        // 2. Create the LLM client
        let client = Arc::new(AgentClient::new(client_config).await?);
        
        // 3. Fetch rulebooks in parallel
        let rulebooks = client.list_rulebooks().await.ok();
        
        // 4. Choose mode: async or interactive
        match use_async_mode {
            true => {
                // Non-interactive: run until done
                agent::run::run_async(config, RunAsyncConfig { ... }).await
            }
            false => {
                // Interactive: launch TUI
                agent::run::run_interactive(config, RunInteractiveConfig {
                    local_context,  // <-- Project context is passed here
                    // ...
                }).await
            }
        }
    }
}
```

### 3.2 Interactive Mode: `mode_interactive.rs`

This file orchestrates the TUI and backend communication.

**Location**: `cli/src/commands/agent/run/mode_interactive.rs`

#### 3.2.1 Channel Architecture

The system uses **Tokio channels** for communication:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          mode_interactive.rs                             │
│                                                                          │
│  ┌──────────────────┐    input_tx     ┌───────────────────────────────┐ │
│  │                  │  ───────────►   │                               │ │
│  │   Client Task    │                 │   TUI Task (run_tui)          │ │
│  │  (LLM, tools)    │  ◄───────────   │                               │ │
│  │                  │    output_tx    │                               │ │
│  └──────────────────┘                 └───────────────────────────────┘ │
│                                                                          │
│  InputEvent examples:                 OutputEvent examples:              │
│  • UserMessage("hello")               • AcceptTool(tool_call)           │
│  • ToolResult(result)                 • RejectTool(tool_call)           │
│  • StreamChunk(text)                  • Quit                            │
│  • Quit                               • SwitchModel(model)              │
└─────────────────────────────────────────────────────────────────────────┘
```

#### 3.2.2 Spawning the TUI

```rust
// cli/src/commands/agent/run/mode_interactive.rs, lines 191-212

let tui_handle = tokio::spawn(async move {
    let latest_version = get_latest_cli_version().await;
    stakpak_tui::run_tui(
        input_rx,               // Receives events FROM client task
        output_tx,              // Sends events TO client task
        Some(cancel_tx.clone()),
        shutdown_tx_for_tui,
        latest_version.ok(),
        redact_secrets,
        privacy_mode,
        is_git_repo,
        auto_approve.as_ref(),
        allowed_tools.as_ref(),
        current_profile_for_tui,
        rulebook_config_for_tui,
        model_for_tui,
        editor_command,
        auth_display_info_for_tui,
    )
    .await
    .map_err(|e| e.to_string())
});
```

**Problem identified**: `local_context` is available here but is **NOT** passed to `run_tui`!

---

## 4. The TUI Crate Deep Dive

### 4.1 Entry Point: `tui/src/lib.rs`

```rust
// tui/src/lib.rs

pub use app::{AppState, InputEvent, LoadingOperation, OutputEvent, SessionInfo};
pub use event_loop::{RulebookConfig, run_tui};
```

The TUI is a **library**, not a binary. It exports `run_tui` which the CLI calls.

### 4.2 AppState: The Central State Container

**Location**: `tui/src/app.rs`

```rust
// tui/src/app.rs (simplified)

pub struct AppState {
    // === Input Area ===
    pub text_area: TextArea,           // The input box at the bottom
    pub show_helper_dropdown: bool,    // Is the dropdown visible?
    pub filtered_helpers: Vec<HelperCommand>,  // Current dropdown options
    
    // === Messages (Chat History) ===
    pub messages: Vec<Message>,        // All rendered messages
    pub scroll_offset: usize,          // Current scroll position
    
    // === Tool Calls ===
    pub tool_call_under_review: Option<ToolCall>,  // Tool waiting for approval
    pub session_tool_calls_queue: HashMap<String, ToolCallStatus>,
    
    // === Popups ===
    pub show_model_switcher: bool,
    pub show_profile_switcher: bool,
    pub show_shortcuts: bool,
    
    // === Shell Mode ===
    pub active_shell_command: Option<ShellCommand>,
    pub shell_popup_visible: bool,
    
    // === Status ===
    pub loading_operations: HashSet<LoadingOperation>,
    pub status_message: Option<String>,
    
    // === Configuration ===
    pub current_profile_name: String,
    pub model: Model,
    
    // ... many more fields
}
```

**Key insight**: `AppState` holds ALL the TUI state. If you want something accessible in slash commands, it needs to be in `AppState`.

### 4.3 The Event Loop: `tui/src/event_loop.rs`

This is the heart of the TUI. It's an infinite loop that:
1. Waits for events (keyboard, mouse, incoming messages)
2. Updates state
3. Redraws the UI

```rust
// tui/src/event_loop.rs (simplified)

pub async fn run_tui(
    mut input_rx: Receiver<InputEvent>,     // Events from CLI
    output_tx: Sender<OutputEvent>,          // Events to CLI
    // ... other parameters
) -> io::Result<()> {
    // 1. Initialize terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    
    // 2. Create state
    let mut state = AppState::new(AppStateOptions { /* ... */ });
    
    // 3. Main loop
    loop {
        tokio::select! {
            // Case A: Event from CLI (InputEvent)
            event = input_rx.recv() => {
                // Update state based on event
                handle_input_event(&mut state, event, &output_tx);
            }
            
            // Case B: Keyboard/mouse event
            event = internal_rx.recv() => {
                // Process user input
                handle_keyboard_event(&mut state, event, &output_tx);
            }
            
            // Case C: Timer tick (for animations)
            _ = spinner_interval.tick() => {
                state.spinner_frame = state.spinner_frame.wrapping_add(1);
            }
        }
        
        // 4. Redraw UI
        terminal.draw(|f| view(f, &mut state))?;
        
        if should_quit {
            break;
        }
    }
    
    Ok(())
}
```

### 4.4 Services Directory Structure

```
tui/src/services/
├── commands.rs           # ⭐ SLASH COMMAND REGISTRY & EXECUTION
├── handlers/
│   ├── input.rs          # Keyboard/mouse input handling
│   ├── shell.rs          # Interactive shell command handling
│   ├── tool.rs           # Tool call result handling
│   ├── dialog.rs         # Confirmation dialogs
│   ├── popup.rs          # Popup navigation
│   └── mod.rs            # Main update dispatcher
├── message.rs            # Message rendering
├── markdown_renderer.rs  # Markdown → TUI rendering
├── textarea.rs           # Input box component
├── helper_dropdown.rs    # Autocomplete dropdown
└── ... (30+ more files)
```

---

## 5. Slash Command System

### 5.1 What is a Slash Command?

A slash command is a TUI-internal command that starts with `/`. When the user types `/help` and presses Enter, the TUI executes a predefined action.

**Examples**:
- `/help` - Show help message
- `/clear` - Clear the screen
- `/model` - Open model switcher
- `/profiles` - Switch profiles
- `/quit` - Exit the application

### 5.2 Command Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              User types "/help"                              │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Step 1: Input Detection (services/handlers/input.rs)                        │
│                                                                              │
│ When user types, the helper dropdown filters available commands.            │
│ When Enter is pressed, if text starts with "/", it's treated as a command. │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Step 2: Command Lookup (services/commands.rs)                               │
│                                                                              │
│ The input is matched against registered commands.                           │
│ commands_to_helper_commands() provides the list for the dropdown.           │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Step 3: Command Execution (services/commands.rs::execute_command)           │
│                                                                              │
│ pub fn execute_command(command_id: CommandId, ctx: CommandContext)          │
│                                                                              │
│ A big match statement dispatches to the appropriate handler:                │
│                                                                              │
│ match command_id {                                                          │
│     "/help" => { push_help_message(ctx.state); Ok(()) }                     │
│     "/model" => { ctx.state.show_model_switcher = true; Ok(()) }            │
│     "/clear" => { push_clear_message(ctx.state); Ok(()) }                   │
│     // ... more commands                                                    │
│     _ => Err(format!("Unknown command: {}", command_id)),                   │
│ }                                                                           │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Step 4: State Update                                                         │
│                                                                              │
│ The command modifies AppState (e.g., adds messages, opens popups).          │
│ The event loop redraws the UI to reflect the changes.                       │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.3 The Command Registry

**Location**: `tui/src/services/commands.rs`

#### 5.3.1 Command Struct

```rust
// For the Command Palette UI
pub struct Command {
    pub name: String,        // "Help"
    pub description: String, // "Show help information"
    pub shortcut: String,    // "/help"
    pub action: CommandAction,
}
```

#### 5.3.2 HelperCommand Struct

```rust
// For the autocomplete dropdown
pub struct HelperCommand {
    pub command: &'static str,    // "/help"
    pub description: &'static str, // "Show help information and available commands"
}
```

#### 5.3.3 CommandContext

```rust
// Passed to every command handler
pub struct CommandContext<'a> {
    pub state: &'a mut AppState,              // Mutable access to all state
    pub input_tx: &'a Sender<InputEvent>,     // Send events to internal loop
    pub output_tx: &'a Sender<OutputEvent>,   // Send events to CLI backend
}
```

### 5.4 Existing Commands

| Command | Description | Implementation |
|---------|-------------|----------------|
| `/help` | Show help message | `push_help_message(ctx.state)` |
| `/model` | Open model switcher | `ctx.state.show_model_switcher = true` |
| `/clear` | Clear screen | `push_clear_message(ctx.state)` |
| `/status` | Show account status | `push_status_message(ctx.state)` |
| `/sessions` | List sessions | `ctx.output_tx.try_send(OutputEvent::ListSessions)` |
| `/resume` | Resume last session | `resume_session(ctx.state, ctx.output_tx)` |
| `/new` | Start new session | `new_session(ctx.state, ctx.output_tx)` |
| `/memorize` | Save to memory | `ctx.output_tx.try_send(OutputEvent::Memorize)` |
| `/summarize` | Create summary.md | Builds prompt, sends `OutputEvent::UserMessage` |
| `/usage` | Show token usage | `push_usage_message(ctx.state)` |
| `/issue` | Report bug | `push_issue_message(ctx.state)` |
| `/editor` | Open in editor | `ctx.state.pending_editor_open = Some(path)` |
| `/support` | Discord link | `push_support_message(ctx.state)` |
| `/profiles` | Switch profile | `ctx.state.show_profile_switcher = true` |
| `/quit` | Exit app | `ctx.input_tx.try_send(InputEvent::Quit)` |
| `/shortcuts` | Show shortcuts | `ctx.input_tx.try_send(InputEvent::ShowShortcuts)` |
| `/mouse_capture` | Toggle mouse | `ctx.input_tx.try_send(InputEvent::ToggleMouseCapture)` |

---

## 6. Step-by-Step: Adding a New Slash Command

### 6.1 Quick Checklist

To add a new slash command like `/mycommand`:

- [ ] **Step 1**: Add to `commands_to_helper_commands()` in `tui/src/services/commands.rs`
- [ ] **Step 2**: Add case to `execute_command()` match block
- [ ] **Step 3**: (Optional) Add to `get_all_commands()` for Command Palette
- [ ] **Step 4**: Implement any helper functions needed
- [ ] **Step 5**: Add any new state fields to `AppState` if needed

### 6.2 Example: Adding `/hello`

#### Step 1: Register in Dropdown

```rust
// tui/src/services/commands.rs, in commands_to_helper_commands()

pub fn commands_to_helper_commands() -> Vec<HelperCommand> {
    vec![
        // ... existing commands ...
        HelperCommand {
            command: "/hello",
            description: "Say hello to the user",
        },
    ]
}
```

#### Step 2: Add Execution Logic

```rust
// tui/src/services/commands.rs, in execute_command()

pub fn execute_command(command_id: CommandId, ctx: CommandContext) -> Result<(), String> {
    match command_id {
        // ... existing commands ...
        
        "/hello" => {
            // Add a message to the chat
            ctx.state.messages.push(Message::info(
                "👋 Hello! Welcome to Stakpak!",
                Some(Style::default().fg(Color::Green)),
            ));
            
            // Clear the input box
            ctx.state.text_area.set_text("");
            
            // Close the dropdown
            ctx.state.show_helper_dropdown = false;
            
            // Invalidate message cache so new message is rendered
            crate::services::message::invalidate_message_lines_cache(ctx.state);
            
            Ok(())
        }
        
        _ => Err(format!("Unknown command: {}", command_id)),
    }
}
```

#### Step 3: (Optional) Add to Command Palette

```rust
// tui/src/services/commands.rs, in get_all_commands()

pub fn get_all_commands() -> Vec<Command> {
    vec![
        // ... existing commands ...
        Command::new(
            "Hello",
            "Say hello to the user",
            "/hello",
            CommandAction::Custom, // You may need to add this variant
        ),
    ]
}
```

### 6.3 Command Patterns

#### Pattern A: Simple Message

```rust
"/mycommand" => {
    ctx.state.messages.push(Message::info("Hello!", None));
    ctx.state.text_area.set_text("");
    ctx.state.show_helper_dropdown = false;
    Ok(())
}
```

#### Pattern B: Open a Popup

```rust
"/mycommand" => {
    ctx.state.my_popup_visible = true;  // Add this field to AppState
    ctx.state.text_area.set_text("");
    ctx.state.show_helper_dropdown = false;
    Ok(())
}
```

#### Pattern C: Send Event to Backend

```rust
"/mycommand" => {
    let _ = ctx.output_tx.try_send(OutputEvent::MyCustomEvent);  // Add this variant
    ctx.state.text_area.set_text("");
    ctx.state.show_helper_dropdown = false;
    Ok(())
}
```

#### Pattern D: Populate Input with Text

```rust
"/mycommand" => {
    let prompt = "This is a template prompt...";
    ctx.state.text_area.set_text(prompt);
    ctx.state.text_area.set_cursor(prompt.len());
    ctx.state.show_helper_dropdown = false;
    Ok(())
}
```

---

## 7. RFC: `/init` Command

### 7.1 Objective

Crea
2. Suggests relevant DevOps actionste an intelligent initialization command that:
1. Analyzes the current project's technology stack
3. Populates the input box with a context-aware prompt

### 7.2 User Experience

```
$ stakpak

  ┌─────────────────────────────────────────────────────────────────────────┐
  │ Welcome to Stakpak! Type a message or use /help for more information.  │
  └─────────────────────────────────────────────────────────────────────────┘

  > /init

  ┌─────────────────────────────────────────────────────────────────────────┐
  │ I have analyzed the current project:                                    │
  │                                                                         │
  │ # System Details                                                        │
  │ Machine Name: devbox-1234                                               │
  │ Operating System: macOS                                                 │
  │ Working Directory: /Users/dev/my-project                                │
  │ Git Repository: yes                                                     │
  │ Current Branch: main                                                    │
  │                                                                         │
  │ # Detected Technologies                                                 │
  │ ├── Terraform (*.tf files detected)                                    │
  │ ├── Docker (Dockerfile found)                                          │
  │ ├── Kubernetes (k8s/ directory)                                        │
  │ └── AWS (provider "aws" in Terraform)                                  │
  │                                                                         │
  │ Please help me initialize Stakpak for this project. Suggested actions: │
  │ - [ ] Analyze Infrastructure Costs                                      │
  │ - [ ] Set up Monitoring (CloudWatch/Datadog/Prometheus)                 │
  │ - [ ] Initialize Stakpak Watch for continuous monitoring               │
  │ - [ ] Review Security Configuration                                     │
  │                                                                         │
  │ Please execute the selected actions and provide a summary.             │
  └─────────────────────────────────────────────────────────────────────────┘
```

### 7.3 Implementation Plan

#### Phase 1: Add `local_context_summary` to AppState

**File**: `tui/src/app.rs`

```rust
pub struct AppState {
    // ... existing fields ...
    
    /// Summary of the local context for initialization commands
    pub local_context_summary: Option<String>,
}

pub struct AppStateOptions<'a> {
    // ... existing fields ...
    pub local_context_summary: Option<String>,
}
```

**Status**: ⚠️ Partially done (needs verification)

#### Phase 2: Update `run_tui` Signature

**File**: `tui/src/event_loop.rs`

Add parameter:
```rust
pub async fn run_tui(
    // ... existing params ...
    local_context_summary: Option<String>,
) -> io::Result<()>
```

**Status**: ⚠️ Partially done (needs verification)

#### Phase 3: Pass Context from CLI

**File**: `cli/src/commands/agent/run/mode_interactive.rs`

```rust
// Before TUI spawn
let local_context_summary_for_tui = if let Some(ref ctx) = local_context {
    ctx.format_display().await.ok()
} else {
    None
};

// In run_tui call
stakpak_tui::run_tui(
    // ... existing args ...
    local_context_summary_for_tui,
)
```

**Status**: ❌ Not done

#### Phase 4: Register Command

**File**: `tui/src/services/commands.rs`

```rust
// In commands_to_helper_commands()
HelperCommand {
    command: "/init",
    description: "Analyze project and suggest initialization actions",
},
```

**Status**: ❌ Not done

#### Phase 5: Implement Execution Logic

**File**: `tui/src/services/commands.rs`

```rust
"/init" => {
    let context = ctx.state.local_context_summary.as_deref()
        .unwrap_or("No project context detected. Please describe your infrastructure.");

    let prompt = format!(
r#"I have analyzed the current project:

{}

Please help me initialize Stakpak for this project. Suggested actions:
- [ ] Analyze Infrastructure Costs
- [ ] Set up Monitoring (CloudWatch/Datadog/Prometheus)
- [ ] Initialize Stakpak Watch for continuous monitoring
- [ ] Review Security Configuration

Please execute the selected actions and provide a summary."#,
        context
    );

    ctx.state.text_area.set_text(&prompt);
    ctx.state.text_area.set_cursor(prompt.len());
    ctx.state.show_helper_dropdown = false;
    Ok(())
}
```

**Status**: ❌ Not done

### 7.4 Files to Modify Summary

| File | Change | Priority |
|------|--------|----------|
| `tui/src/app.rs` | Add `local_context_summary` field | High |
| `tui/src/event_loop.rs` | Update `run_tui` signature | High |
| `cli/src/commands/agent/run/mode_interactive.rs` | Pass context to TUI | High |
| `tui/src/services/commands.rs` | Add command handler | High |
| `tui/src/services/handlers/mod.rs` | Fix any broken `AppStateOptions` usage | Medium |

### 7.5 Current Build Status

⚠️ **The codebase is currently in an incomplete state**. 
Partial changes were made to `app.rs` and `event_loop.rs`, but the call sites in `mode_interactive.rs` and `handlers/mod.rs` were not updated, causing compilation errors.

**To fix**: Either complete the implementation or revert the partial changes:

```bash
# To revert (if needed)
git checkout -- tui/src/app.rs tui/src/event_loop.rs
```

---

## 8. TUI-Server Communication & Streaming Architecture

This section provides a deep dive into how the TUI communicates with the backend, how LLM responses are streamed, and how tool calls are executed through the MCP (Model Context Protocol) proxy infrastructure.

### 8.1 Overview: The Three-Layer Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│                                    STAKPAK ARCHITECTURE                                  │
├─────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                          │
│  LAYER 1: TUI (Terminal Interface)                                                       │
│  ┌────────────────────────────────────────────────────────────────────────────────────┐ │
│  │ tui/src/event_loop.rs                                                               │ │
│  │                                                                                      │ │
│  │  • Receives InputEvents from Client Task (streamed text, tool calls, status)       │ │
│  │  • Sends OutputEvents to Client Task (user messages, tool approvals, commands)     │ │
│  │  • Manages AppState and renders UI using ratatui                                    │ │
│  └────────────────────────────────────────────────────────────────────────────────────┘ │
│                                      ▲                                                   │
│                                      │ Tokio mpsc channels                               │
│                                      ▼                                                   │
│  LAYER 2: Client Task (Orchestration)                                                    │
│  ┌────────────────────────────────────────────────────────────────────────────────────┐ │
│  │ cli/src/commands/agent/run/mode_interactive.rs                                      │ │
│  │                                                                                      │ │
│  │  • Spawns TUI task and Client task concurrently                                     │ │
│  │  • Manages LLM communication via AgentProvider trait                                │ │
│  │  • Processes streaming responses via stream.rs                                      │ │
│  │  • Executes tool calls via MCP client                                               │ │
│  │  • Manages session state and message history                                        │ │
│  └────────────────────────────────────────────────────────────────────────────────────┘ │
│                                      ▲                                                   │
│                                      │ HTTPS + mTLS                                      │
│                                      ▼                                                   │
│  LAYER 3: MCP Infrastructure                                                             │
│  ┌────────────────────────────────────────────────────────────────────────────────────┐ │
│  │  ┌───────────────┐     ┌─────────────────┐     ┌────────────────────────────────┐ │ │
│  │  │  MCP Client   │────▶│   MCP Proxy     │────▶│   Upstream MCP Servers         │ │ │
│  │  │ libs/mcp/     │     │ libs/mcp/proxy  │     │  ├── stakpak (local tools)     │ │ │
│  │  │ client        │     │                 │     │  └── paks (cloud tools)        │ │ │
│  │  └───────────────┘     └─────────────────┘     └────────────────────────────────┘ │ │
│  └────────────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                          │
└─────────────────────────────────────────────────────────────────────────────────────────┘
```

### 8.2 Channel Communication Deep Dive

#### 8.2.1 The Two-Way Channel System

The TUI and Client Task communicate via **Tokio mpsc channels**:

```rust
// cli/src/commands/agent/run/mode_interactive.rs, lines 171-175

let (input_tx, input_rx) = tokio::sync::mpsc::channel::<InputEvent>(100);
let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<OutputEvent>(100);
let (mcp_progress_tx, mut mcp_progress_rx) = tokio::sync::mpsc::channel(100);
let (shutdown_tx, _shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);
let (cancel_tx, cancel_rx) = tokio::sync::broadcast::channel::<()>(1);
```

**Key channels:**

| Channel | Direction | Purpose |
|---------|-----------|---------|
| `input_tx` / `input_rx` | Client → TUI | Streaming text, tool calls, status updates |
| `output_tx` / `output_rx` | TUI → Client | User messages, tool approvals, commands |
| `mcp_progress_tx` / `mcp_progress_rx` | MCP → TUI | Real-time tool execution progress |
| `shutdown_tx` / `shutdown_rx` | Broadcast | Graceful shutdown signal |
| `cancel_tx` / `cancel_rx` | Broadcast | Cancel current operation |

#### 8.2.2 InputEvent Types (Client → TUI)

```rust
// tui/src/app.rs (simplified)

pub enum InputEvent {
    // === Streaming LLM responses ===
    StreamAssistantMessage(StreamDelta),  // Streamed text chunks
    StreamToolCalls(Vec<ToolCallDelta>),  // Streamed tool call deltas
    MessageToolCalls(Vec<ToolCall>),      // Complete tool calls for display
    
    // === Tool execution ===
    StreamToolResult(ToolCallResultProgress),  // Real-time tool progress
    ToolCallResult(ToolCall, Result<...>),     // Tool execution result
    
    // === Status updates ===
    GetStatus(String),           // Account status
    BillingInfo(BillingInfo),    // Token usage/billing
    UsageUpdate(LLMTokenUsage),  // Per-request token usage
    
    // === Session management ===
    ProfilesLoaded(Vec<String>, String),  // Available profiles
    RulebooksLoaded(Vec<ListRuleBook>),   // Available rulebooks
    SessionsLoaded(Vec<Session>),         // Available sessions
    
    // === UI Control ===
    StartLoadingOperation(LoadingOperation),
    EndLoadingOperation(LoadingOperation),
    ShowShortcuts,
    Quit,
}
```

#### 8.2.3 OutputEvent Types (TUI → Client)

```rust
// tui/src/app.rs (simplified)

pub enum OutputEvent {
    // === User actions ===
    UserMessage(String),         // User submits a message
    AcceptTool(ToolCall),        // User approves tool execution
    RejectTool(ToolCall),        // User rejects tool execution
    CancelCurrentRun,            // User cancels current LLM call
    
    // === Session control ===
    SwitchModel(Model),          // Change LLM model
    SwitchProfile(String),       // Change config profile
    ListSessions,                // Request session list
    ResumeSession(Uuid),         // Resume a previous session
    NewSession,                  // Start fresh session
    
    // === Other ===
    Memorize,                    // Save context to memory
    Quit,                        // Exit application
}
```

### 8.3 Streaming Response Processing

#### 8.3.1 The AgentProvider Trait

All LLM providers implement this trait:

```rust
// libs/api/src/lib.rs

#[async_trait]
pub trait AgentProvider: Send + Sync {
    /// Streaming chat completion - returns a stream of deltas
    async fn chat_completion_stream(
        &self,
        model: Model,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        headers: Option<HeaderMap>,
        session_id: Option<Uuid>,
    ) -> Result<(
        Pin<Box<dyn Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>> + Send>>,
        Option<String>,  // Request ID
    ), String>;
    
    /// Non-streaming (for simple requests)
    async fn chat_completion(...) -> Result<ChatCompletionResponse, String>;
}
```

#### 8.3.2 Stream Processing Flow

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                              STREAMING RESPONSE FLOW                                 │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│  1. LLM Provider (Claude/GPT/Gemini)                                                │
│     │                                                                               │
│     │  Sends Server-Sent Events (SSE):                                              │
│     │  data: {"choices": [{"delta": {"content": "Hello"}}]}                        │
│     │  data: {"choices": [{"delta": {"content": " world"}}]}                       │
│     │  data: {"choices": [{"delta": {"tool_calls": [...]}}]}                       │
│     ▼                                                                               │
│                                                                                      │
│  2. AgentClient (libs/api)                                                          │
│     │                                                                               │
│     │  Parses SSE into ChatCompletionStreamResponse                                │
│     │  Returns Pin<Box<dyn Stream<...>>>                                           │
│     ▼                                                                               │
│                                                                                      │
│  3. process_responses_stream() (cli/src/commands/agent/run/stream.rs)              │
│     │                                                                               │
│     │  ┌────────────────────────────────────────────────────────────────────────┐  │
│     │  │  loop {                                                                 │  │
│     │  │      let response = stream.next().await;                               │  │
│     │  │                                                                         │  │
│     │  │      // Accumulate text content                                        │  │
│     │  │      if let Some(text) = response.choices[0].delta.content {           │  │
│     │  │          accumulated_content.push_str(&text);                          │  │
│     │  │          input_tx.send(InputEvent::StreamAssistantMessage(delta));     │  │
│     │  │      }                                                                  │  │
│     │  │                                                                         │  │
│     │  │      // Process tool call deltas                                       │  │
│     │  │      if let Some(tool_calls) = response.choices[0].delta.tool_calls {  │  │
│     │  │          tool_call_accumulator.process_delta(&tool_call);              │  │
│     │  │          input_tx.send(InputEvent::StreamToolCalls(deltas));           │  │
│     │  │      }                                                                  │  │
│     │  │  }                                                                      │  │
│     │  └────────────────────────────────────────────────────────────────────────┘  │
│     ▼                                                                               │
│                                                                                      │
│  4. TUI Event Loop (tui/src/event_loop.rs)                                          │
│     │                                                                               │
│     │  Receives InputEvents and updates AppState:                                  │
│     │  - Appends text to current message                                           │
│     │  - Updates tool call display                                                 │
│     │  - Triggers UI redraw                                                        │
│     ▼                                                                               │
│                                                                                      │
│  5. User sees real-time streaming in terminal                                       │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

#### 8.3.3 Tool Call Accumulator

LLM providers stream tool calls in fragments. The `ToolCallAccumulator` reassembles them:

```rust
// cli/src/commands/agent/run/stream.rs

pub struct ToolCallAccumulator {
    tool_calls: Vec<ToolCall>,
}

impl ToolCallAccumulator {
    /// Process a streaming delta and accumulate into complete tool calls
    pub fn process_delta(&mut self, delta: &ToolCallDelta) {
        // Handle two provider behaviors:
        // 1. ID-based matching (Claude): Each delta has a unique ID
        // 2. Index-based matching (OpenAI): Deltas reference position index
        
        if let Some(existing) = self.find_tool_call(delta.id.as_deref(), delta.index) {
            // Append to existing tool call
            if let Some(args) = &delta.function.arguments {
                existing.function.arguments.push_str(args);
            }
        } else {
            // Create new tool call
            self.create_tool_call(delta);
        }
    }
    
    /// Get the complete, assembled tool calls
    pub fn into_tool_calls(self) -> Vec<ToolCall> {
        self.tool_calls
    }
}
```

**Example streaming tool call:**

```
// Delta 1: { index: 0, id: "call_123", function: { name: "read_file", arguments: "" } }
// Delta 2: { index: 0, id: "call_123", function: { arguments: "{\"path\"" } }
// Delta 3: { index: 0, id: "call_123", function: { arguments: ": \"/src\"}" } }
//
// Accumulated: { id: "call_123", function: { name: "read_file", arguments: "{\"path\": \"/src\"}" } }
```

### 8.4 MCP (Model Context Protocol) Infrastructure

#### 8.4.1 What is MCP?

MCP is Anthropic's open-source protocol for connecting AI systems with tools and data sources. Stakpak uses MCP to:
- Expose local file system tools (read, write, search)
- Integrate with external paks (Stakpak cloud tools)
- Manage secret redaction and security

#### 8.4.2 MCP Initialization

```rust
// cli/src/commands/agent/run/mcp_init.rs

/// Initialize the complete MCP infrastructure
pub async fn initialize_mcp_server_and_tools(
    app_config: &AppConfig,
    mcp_config: McpInitConfig,
    progress_tx: Option<Sender<ToolCallResultProgress>>,
) -> Result<McpInitResult, String> {
    // 1. Generate certificate chains for mTLS
    let certs = CertificateChains::generate()?;
    
    // 2. Find available ports for local servers
    let server_binding = ServerBinding::new("MCP server").await?;
    let proxy_binding = ServerBinding::new("proxy").await?;
    
    // 3. Start local MCP server (provides file system tools)
    start_mcp_server(app_config, &mcp_config, server_binding, ...).await?;
    
    // 4. Build proxy config with upstream servers
    let pool_config = build_proxy_config(
        local_mcp_server_url,
        // Adds:
        // - "stakpak": local tools at https://localhost:PORT/mcp
        // - "paks": cloud tools at https://apiv2.stakpak.dev/v1/paks/mcp
    );
    
    // 5. Start proxy server (aggregates all MCP servers)
    start_proxy(pool_config, &mcp_config, proxy_binding, ...).await?;
    
    // 6. Connect client to proxy with retry logic
    let mcp_client = connect_to_proxy(&proxy_url, certs.proxy_chain, progress_tx).await?;
    
    // 7. Get tool list from MCP client
    let mcp_tools = stakpak_mcp_client::get_tools(&mcp_client).await?;
    
    Ok(McpInitResult {
        client: mcp_client,
        mcp_tools,
        tools,
        server_shutdown_tx,
        proxy_shutdown_tx,
    })
}
```

#### 8.4.3 The MCP Proxy Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│                                   MCP PROXY ARCHITECTURE                                 │
├─────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                          │
│  ┌────────────────────┐                                                                  │
│  │    MCP Client      │                                                                  │
│  │  (libs/mcp/client) │                                                                  │
│  └─────────┬──────────┘                                                                  │
│            │                                                                             │
│            │ HTTPS + mTLS (proxy_chain certificates)                                    │
│            │                                                                             │
│            ▼                                                                             │
│  ┌────────────────────────────────────────────────────────────────────────────────────┐ │
│  │                           MCP Proxy Server                                          │ │
│  │                        (libs/mcp/proxy/src/server)                                  │ │
│  │                                                                                      │ │
│  │  Responsibilities:                                                                   │ │
│  │  • Aggregate tools from multiple upstream servers                                   │ │
│  │  • Prefix tool names with server name (e.g., "stakpak__view_file")                 │ │
│  │  • Route tool calls to correct upstream server                                      │ │
│  │  • Redact secrets in tool responses (using SecretManager)                          │ │
│  │  • Restore secrets in tool arguments before forwarding                              │ │
│  │  • Handle cancellation forwarding                                                   │ │
│  │                                                                                      │ │
│  │  Tool name format: "{server}__{tool}"                                               │ │
│  │  Example: "stakpak__edit_file", "paks__deploy_cloudformation"                       │ │
│  └────────────────────────────────────────────────────────────────────────────────────┘ │
│            │                                                                             │
│     ┌──────┴──────────────────────────────────────────┐                                 │
│     │                                                  │                                 │
│     ▼                                                  ▼                                 │
│  ┌──────────────────────────┐            ┌──────────────────────────────────┐          │
│  │   Local MCP Server       │            │   Remote MCP Server (paks)        │          │
│  │ (libs/mcp/server)        │            │   https://apiv2.stakpak.dev/...   │          │
│  │                          │            │                                    │          │
│  │ Tools:                   │            │ Tools:                             │          │
│  │ • view_file              │            │ • deploy_cloudformation            │          │
│  │ • edit_file              │            │ • analyze_terraform_costs          │          │
│  │ • search_files           │            │ • scan_vulnerabilities             │          │
│  │ • execute_command        │            │ • ... (cloud-based tools)          │          │
│  │ • read_directory         │            │                                    │          │
│  └──────────────────────────┘            └──────────────────────────────────┘          │
│                                                                                          │
└─────────────────────────────────────────────────────────────────────────────────────────┘
```

#### 8.4.4 Tool Call Execution Flow

```
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│                              TOOL CALL EXECUTION FLOW                                    │
├─────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                          │
│  1. LLM generates tool call: stakpak__view_file(path="/src/main.rs")                   │
│     │                                                                                    │
│     ▼                                                                                    │
│                                                                                          │
│  2. Client Task receives complete tool call from stream                                  │
│     │ (cli/src/commands/agent/run/mode_interactive.rs)                                  │
│     │                                                                                    │
│     │ Sends to TUI: InputEvent::MessageToolCalls(vec![tool_call])                       │
│     ▼                                                                                    │
│                                                                                          │
│  3. TUI displays tool call for user review                                               │
│     │                                                                                    │
│     │ User can: [Accept] or [Reject] or [Auto-approve enabled]                          │
│     ▼                                                                                    │
│                                                                                          │
│  4. User approves → TUI sends: OutputEvent::AcceptTool(tool_call)                       │
│     │                                                                                    │
│     ▼                                                                                    │
│                                                                                          │
│  5. Client Task executes via MCP Client                                                  │
│     │                                                                                    │
│     │ run_tool_call(mcp_client, &tool_call, progress_tx).await                          │
│     │                                                                                    │
│     │ ┌────────────────────────────────────────────────────────────────────────────┐    │
│     │ │  MCP Client → Proxy:                                                        │    │
│     │ │  POST /mcp/call_tool                                                        │    │
│     │ │  { "name": "stakpak__view_file", "arguments": {"path": "/src/main.rs"} }  │    │
│     │ └────────────────────────────────────────────────────────────────────────────┘    │
│     ▼                                                                                    │
│                                                                                          │
│  6. Proxy parses tool name and routes to correct upstream                               │
│     │                                                                                    │
│     │ (client_name, tool_name) = parse_tool_name("stakpak__view_file")                 │
│     │ // → ("stakpak", "view_file")                                                    │
│     │                                                                                    │
│     │ Restores any [REDACTED_SECRET:...] placeholders in arguments                     │
│     ▼                                                                                    │
│                                                                                          │
│  7. Proxy calls local MCP Server                                                         │
│     │                                                                                    │
│     │ ┌────────────────────────────────────────────────────────────────────────────┐    │
│     │ │  Proxy → Local Server:                                                       │    │
│     │ │  POST /mcp/call_tool                                                        │    │
│     │ │  { "name": "view_file", "arguments": {"path": "/src/main.rs"} }            │    │
│     │ └────────────────────────────────────────────────────────────────────────────┘    │
│     ▼                                                                                    │
│                                                                                          │
│  8. Local MCP Server executes tool and returns result                                   │
│     │                                                                                    │
│     │ File contents: "fn main() { println!(\"Hello\"); }"                              │
│     ▼                                                                                    │
│                                                                                          │
│  9. Proxy redacts secrets in response using SecretManager                               │
│     │                                                                                    │
│     │ api_key="sk-123" → "[REDACTED_SECRET:api_key:abc123]"                            │
│     ▼                                                                                    │
│                                                                                          │
│  10. Client Task receives result and sends to TUI                                        │
│      │                                                                                   │
│      │ InputEvent::ToolCallResult(tool_call, Ok(result))                               │
│      ▼                                                                                   │
│                                                                                          │
│  11. TUI displays result and adds to message history                                     │
│      │                                                                                   │
│      │ Result is also added to LLM context for next turn                               │
│      ▼                                                                                   │
│                                                                                          │
│  12. Loop continues with updated context                                                 │
│                                                                                          │
└─────────────────────────────────────────────────────────────────────────────────────────┘
```

#### 8.4.5 Real-Time Tool Progress

Tools can report progress during execution:

```rust
// libs/shared/src/models/integrations/openai.rs

pub struct ToolCallResultProgress {
    pub tool_call_id: String,
    pub progress: String,  // "Reading file...", "Writing 2/5 files..."
}
```

Progress is streamed via the MCP progress channel:

```rust
// cli/src/commands/agent/run/mode_interactive.rs, lines 216-234

let mcp_progress_handle = tokio::spawn(async move {
    loop {
        tokio::select! {
            maybe_progress = mcp_progress_rx.recv() => {
                let Some(progress) = maybe_progress else { break; };
                let _ = send_input_event(
                    &input_tx_clone,
                    InputEvent::StreamToolResult(progress),
                ).await;
            }
            _ = shutdown_rx_for_progress.recv() => { break; }
        }
    }
});
```

### 8.5 Security: mTLS and Secret Redaction

#### 8.5.1 mTLS Certificate Chains

The system uses mTLS (mutual TLS) for secure local communication:

```rust
// cli/src/commands/agent/run/mcp_init.rs

struct CertificateChains {
    /// Certificate chain for MCP Server ↔ Proxy communication
    server_chain: Arc<Option<CertificateChain>>,
    /// Certificate chain for Proxy ↔ Client communication  
    proxy_chain: Arc<CertificateChain>,
}

impl CertificateChains {
    fn generate() -> Result<Self, String> {
        // Generates ephemeral certificates at startup
        // Both client and server must present valid certificates
        let server_chain = CertificateChain::generate()?;
        let proxy_chain = CertificateChain::generate()?;
        Ok(Self { server_chain, proxy_chain })
    }
}
```

**Why two chains?**
- `server_chain`: Authenticates communication between proxy and local MCP server
- `proxy_chain`: Authenticates communication between MCP client and proxy

#### 8.5.2 Secret Redaction

The `SecretManager` protects sensitive data:

```rust
// libs/mcp/proxy/src/server/mod.rs

impl ProxyServer {
    /// Redact secrets in content items before sending to LLM
    fn redact_content(&self, content: Vec<Content>) -> Vec<Content> {
        content.into_iter().map(|item| {
            if let Some(text_content) = item.raw.as_text() {
                let redacted = self.secret_manager
                    .redact_and_store_secrets(&text_content.text, None);
                Content::text(&redacted)
            } else {
                item
            }
        }).collect()
    }
    
    /// Restore secrets in tool arguments before executing
    fn prepare_tool_params(&self, params: &CallToolRequestParam, ...) -> CallToolRequestParam {
        let redaction_map = self.secret_manager.load_session_redaction_map();
        // Walk JSON tree and restore [REDACTED_SECRET:...] placeholders
        restore_secrets_in_json_value(&mut tool_params.arguments, &redaction_map);
        tool_params
    }
}
```

**Redaction flow:**
1. Tool returns content containing `api_key="sk-live-abc123"`
2. Proxy redacts: `api_key="[REDACTED_SECRET:pw:xyz789]"`
3. LLM sees only the redacted version
4. When LLM uses the secret in a tool call, proxy restores original value

### 8.6 Key Files Reference (Streaming & MCP)

| Purpose | File Path |
|---------|-----------|
| Stream processing | `cli/src/commands/agent/run/stream.rs` |
| Tool call accumulator | `cli/src/commands/agent/run/stream.rs` |
| MCP initialization | `cli/src/commands/agent/run/mcp_init.rs` |
| AgentProvider trait | `libs/api/src/lib.rs` |
| MCP client | `libs/mcp/client/src/lib.rs` |
| MCP server (tools) | `libs/mcp/server/src/lib.rs` |
| MCP proxy server | `libs/mcp/proxy/src/server/mod.rs` |
| Secret management | `libs/shared/src/secret_manager.rs` |
| Certificate utilities | `libs/shared/src/cert_utils.rs` |

---

## Appendix: Key Files Quick Reference

| Purpose | File Path |
|---------|-----------|
| CLI entry point & arg parsing | `cli/src/main.rs` |
| Configuration loading | `cli/src/config/mod.rs` |
| Local context analysis | `cli/src/utils/local_context.rs` |
| Interactive mode orchestration | `cli/src/commands/agent/run/mode_interactive.rs` |
| TUI event loop | `tui/src/event_loop.rs` |
| TUI state container | `tui/src/app.rs` |
| **Slash command registry** | `tui/src/services/commands.rs` |
| Keyboard input handling | `tui/src/services/handlers/input.rs` |
| Helper dropdown | `tui/src/services/helper_dropdown.rs` |
| Message rendering | `tui/src/services/message.rs` |
| Stakpak config file | `~/.stakpak/config.toml` |
