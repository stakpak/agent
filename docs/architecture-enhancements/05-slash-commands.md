# Enhancement Proposal: Slash Commands System

## Overview

OpenCode implements a slash command system (`/connect`, `/undo`, `/redo`, `/share`) that provides quick access to common actions within the TUI. Stakpak currently uses keyboard shortcuts and menus for similar functionality.

## Current Stakpak Input Handling

```rust
// tui/src/services/handlers/input.rs
pub async fn handle_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Cancel
        }
        KeyCode::F(1) => {
            // Help
        }
        // ... more keyboard shortcuts
    }
}
```

## OpenCode Slash Commands

```typescript
// Tips showing available commands
const TIPS = [
  "Run {highlight}/connect{/highlight} to add API keys for 75+ supported LLM providers.",
  "Use {highlight}/undo{/highlight} to revert the last message and any file changes made by OpenCode.",
  "Use {highlight}/redo{/highlight} to restore previously undone messages and file changes.",
  "Run {highlight}/share{/highlight} to create a public link to your conversation at opencode.ai.",
]
```

## Proposed Enhancement

### Command Registry

```rust
// libs/shared/src/commands/mod.rs
use std::collections::HashMap;

pub trait SlashCommand: Send + Sync {
    fn name(&self) -> &str;
    fn aliases(&self) -> Vec<&str> { vec![] }
    fn description(&self) -> &str;
    fn usage(&self) -> &str { self.name() }
    fn execute(&self, ctx: &mut CommandContext, args: &[&str]) -> Result<CommandResult>;
    fn autocomplete(&self, _partial: &str) -> Vec<String> { vec![] }
}

pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn SlashCommand>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            commands: HashMap::new(),
        };
        
        // Register built-in commands
        registry.register(Box::new(ConnectCommand));
        registry.register(Box::new(UndoCommand));
        registry.register(Box::new(RedoCommand));
        registry.register(Box::new(ClearCommand));
        registry.register(Box::new(HelpCommand));
        registry.register(Box::new(ModelCommand));
        registry.register(Box::new(SessionCommand));
        registry.register(Box::new(ExportCommand));
        registry.register(Box::new(ConfigCommand));
        
        registry
    }
    
    pub fn register(&mut self, cmd: Box<dyn SlashCommand>) {
        self.commands.insert(cmd.name().to_string(), cmd);
    }
    
    pub fn execute(&self, ctx: &mut CommandContext, input: &str) -> Result<CommandResult> {
        let parts: Vec<&str> = input.trim_start_matches('/').split_whitespace().collect();
        let (name, args) = parts.split_first()
            .ok_or_else(|| anyhow!("Empty command"))?;
        
        let cmd = self.commands.get(*name)
            .or_else(|| self.find_by_alias(name))
            .ok_or_else(|| anyhow!("Unknown command: {}", name))?;
        
        cmd.execute(ctx, args)
    }
    
    pub fn autocomplete(&self, partial: &str) -> Vec<String> {
        let partial = partial.trim_start_matches('/');
        self.commands.keys()
            .filter(|name| name.starts_with(partial))
            .map(|name| format!("/{}", name))
            .collect()
    }
    
    pub fn list(&self) -> Vec<(&str, &str)> {
        self.commands.values()
            .map(|cmd| (cmd.name(), cmd.description()))
            .collect()
    }
}
```

### Built-in Commands

```rust
// libs/shared/src/commands/builtin.rs

// /connect - Provider authentication
pub struct ConnectCommand;

impl SlashCommand for ConnectCommand {
    fn name(&self) -> &str { "connect" }
    fn aliases(&self) -> Vec<&str> { vec!["login", "auth"] }
    fn description(&self) -> &str { "Connect to an AI provider" }
    fn usage(&self) -> &str { "/connect [provider]" }
    
    fn execute(&self, ctx: &mut CommandContext, args: &[&str]) -> Result<CommandResult> {
        if args.is_empty() {
            // Show provider selection menu
            return Ok(CommandResult::ShowProviderMenu);
        }
        
        let provider = args[0];
        ctx.event_bus.publish("command.connect", provider)?;
        Ok(CommandResult::Success(format!("Connecting to {}...", provider)))
    }
    
    fn autocomplete(&self, partial: &str) -> Vec<String> {
        let providers = ["anthropic", "openai", "gemini", "openrouter"];
        providers.iter()
            .filter(|p| p.starts_with(partial))
            .map(|p| p.to_string())
            .collect()
    }
}

// /undo - Revert last action
pub struct UndoCommand;

impl SlashCommand for UndoCommand {
    fn name(&self) -> &str { "undo" }
    fn description(&self) -> &str { "Undo the last message and file changes" }
    
    fn execute(&self, ctx: &mut CommandContext, _args: &[&str]) -> Result<CommandResult> {
        let undone = ctx.session.undo()?;
        if undone {
            Ok(CommandResult::Success("Undone last action".to_string()))
        } else {
            Ok(CommandResult::Info("Nothing to undo".to_string()))
        }
    }
}

// /redo - Restore undone action
pub struct RedoCommand;

impl SlashCommand for RedoCommand {
    fn name(&self) -> &str { "redo" }
    fn description(&self) -> &str { "Redo previously undone action" }
    
    fn execute(&self, ctx: &mut CommandContext, _args: &[&str]) -> Result<CommandResult> {
        let redone = ctx.session.redo()?;
        if redone {
            Ok(CommandResult::Success("Redone action".to_string()))
        } else {
            Ok(CommandResult::Info("Nothing to redo".to_string()))
        }
    }
}

// /model - Switch AI model
pub struct ModelCommand;

impl SlashCommand for ModelCommand {
    fn name(&self) -> &str { "model" }
    fn aliases(&self) -> Vec<&str> { vec!["m"] }
    fn description(&self) -> &str { "Switch AI model" }
    fn usage(&self) -> &str { "/model [smart|eco|model-name]" }
    
    fn execute(&self, ctx: &mut CommandContext, args: &[&str]) -> Result<CommandResult> {
        if args.is_empty() {
            return Ok(CommandResult::ShowModelMenu);
        }
        
        let model = args[0];
        ctx.config.set_model(model)?;
        Ok(CommandResult::Success(format!("Switched to {}", model)))
    }
    
    fn autocomplete(&self, partial: &str) -> Vec<String> {
        let models = ["smart", "eco", "claude-sonnet-4", "gpt-4o", "gemini-2.5-pro"];
        models.iter()
            .filter(|m| m.starts_with(partial))
            .map(|m| m.to_string())
            .collect()
    }
}

// /clear - Clear conversation
pub struct ClearCommand;

impl SlashCommand for ClearCommand {
    fn name(&self) -> &str { "clear" }
    fn aliases(&self) -> Vec<&str> { vec!["new", "reset"] }
    fn description(&self) -> &str { "Start a new conversation" }
    
    fn execute(&self, ctx: &mut CommandContext, _args: &[&str]) -> Result<CommandResult> {
        ctx.session.clear()?;
        Ok(CommandResult::Success("Started new conversation".to_string()))
    }
}

// /help - Show available commands
pub struct HelpCommand;

impl SlashCommand for HelpCommand {
    fn name(&self) -> &str { "help" }
    fn aliases(&self) -> Vec<&str> { vec!["?", "commands"] }
    fn description(&self) -> &str { "Show available commands" }
    
    fn execute(&self, ctx: &mut CommandContext, args: &[&str]) -> Result<CommandResult> {
        if let Some(cmd_name) = args.first() {
            // Show help for specific command
            if let Some(cmd) = ctx.registry.get(cmd_name) {
                return Ok(CommandResult::Info(format!(
                    "{}\n\nUsage: {}\n\n{}",
                    cmd.name(),
                    cmd.usage(),
                    cmd.description()
                )));
            }
        }
        
        // Show all commands
        let commands = ctx.registry.list();
        let help_text = commands.iter()
            .map(|(name, desc)| format!("/{:<12} {}", name, desc))
            .collect::<Vec<_>>()
            .join("\n");
        
        Ok(CommandResult::Info(help_text))
    }
}

// /session - Session management
pub struct SessionCommand;

impl SlashCommand for SessionCommand {
    fn name(&self) -> &str { "session" }
    fn aliases(&self) -> Vec<&str> { vec!["s"] }
    fn description(&self) -> &str { "Manage sessions" }
    fn usage(&self) -> &str { "/session [list|load|save|delete] [name]" }
    
    fn execute(&self, ctx: &mut CommandContext, args: &[&str]) -> Result<CommandResult> {
        match args.first().map(|s| *s) {
            Some("list") | None => {
                Ok(CommandResult::ShowSessionList)
            }
            Some("load") => {
                let name = args.get(1).ok_or_else(|| anyhow!("Session name required"))?;
                ctx.session.load(name)?;
                Ok(CommandResult::Success(format!("Loaded session: {}", name)))
            }
            Some("save") => {
                let name = args.get(1);
                ctx.session.save(name)?;
                Ok(CommandResult::Success("Session saved".to_string()))
            }
            Some("delete") => {
                let name = args.get(1).ok_or_else(|| anyhow!("Session name required"))?;
                ctx.session.delete(name)?;
                Ok(CommandResult::Success(format!("Deleted session: {}", name)))
            }
            Some(cmd) => Err(anyhow!("Unknown subcommand: {}", cmd)),
        }
    }
}

// /export - Export conversation
pub struct ExportCommand;

impl SlashCommand for ExportCommand {
    fn name(&self) -> &str { "export" }
    fn description(&self) -> &str { "Export conversation to file" }
    fn usage(&self) -> &str { "/export [format] [filename]" }
    
    fn execute(&self, ctx: &mut CommandContext, args: &[&str]) -> Result<CommandResult> {
        let format = args.first().unwrap_or(&"markdown");
        let filename = args.get(1).map(|s| s.to_string())
            .unwrap_or_else(|| format!("conversation-{}.md", chrono::Utc::now().format("%Y%m%d-%H%M%S")));
        
        ctx.session.export(format, &filename)?;
        Ok(CommandResult::Success(format!("Exported to {}", filename)))
    }
    
    fn autocomplete(&self, partial: &str) -> Vec<String> {
        let formats = ["markdown", "json", "html"];
        formats.iter()
            .filter(|f| f.starts_with(partial))
            .map(|f| f.to_string())
            .collect()
    }
}

// /config - Configuration
pub struct ConfigCommand;

impl SlashCommand for ConfigCommand {
    fn name(&self) -> &str { "config" }
    fn description(&self) -> &str { "View or modify configuration" }
    fn usage(&self) -> &str { "/config [key] [value]" }
    
    fn execute(&self, ctx: &mut CommandContext, args: &[&str]) -> Result<CommandResult> {
        match args.len() {
            0 => Ok(CommandResult::ShowConfigMenu),
            1 => {
                let value = ctx.config.get(args[0])?;
                Ok(CommandResult::Info(format!("{} = {}", args[0], value)))
            }
            _ => {
                ctx.config.set(args[0], args[1])?;
                Ok(CommandResult::Success(format!("Set {} = {}", args[0], args[1])))
            }
        }
    }
}
```

### TUI Integration

```rust
// tui/src/services/commands.rs
use stakpak_shared::commands::{CommandRegistry, CommandContext, CommandResult};

pub struct CommandService {
    registry: CommandRegistry,
}

impl CommandService {
    pub fn new() -> Self {
        Self {
            registry: CommandRegistry::new(),
        }
    }
    
    pub fn is_command(input: &str) -> bool {
        input.trim().starts_with('/')
    }
    
    pub fn handle(&self, app: &mut App, input: &str) -> Result<()> {
        let mut ctx = CommandContext {
            session: &mut app.session,
            config: &mut app.config,
            event_bus: &app.event_bus,
            registry: &self.registry,
        };
        
        match self.registry.execute(&mut ctx, input)? {
            CommandResult::Success(msg) => {
                app.show_notification(&msg, NotificationType::Success);
            }
            CommandResult::Info(msg) => {
                app.show_info_popup(&msg);
            }
            CommandResult::ShowProviderMenu => {
                app.show_provider_menu();
            }
            CommandResult::ShowModelMenu => {
                app.show_model_menu();
            }
            CommandResult::ShowSessionList => {
                app.show_session_dialog();
            }
            CommandResult::ShowConfigMenu => {
                app.show_config_menu();
            }
        }
        
        Ok(())
    }
    
    pub fn autocomplete(&self, partial: &str) -> Vec<String> {
        if !partial.starts_with('/') {
            return vec![];
        }
        
        let parts: Vec<&str> = partial.split_whitespace().collect();
        
        if parts.len() <= 1 {
            // Autocomplete command name
            self.registry.autocomplete(partial)
        } else {
            // Autocomplete command arguments
            let cmd_name = parts[0].trim_start_matches('/');
            if let Some(cmd) = self.registry.get(cmd_name) {
                let arg_partial = parts.last().unwrap_or(&"");
                cmd.autocomplete(arg_partial)
            } else {
                vec![]
            }
        }
    }
}
```

### Input Handler Integration

```rust
// tui/src/services/handlers/input.rs
pub async fn handle_input(app: &mut App, input: &str) -> Result<()> {
    if CommandService::is_command(input) {
        app.command_service.handle(app, input)?;
    } else if input.starts_with('!') {
        // Shell command
        app.execute_shell(&input[1..])?;
    } else {
        // Regular message
        app.send_message(input).await?;
    }
    
    Ok(())
}
```

## Available Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `/connect` | `/login`, `/auth` | Connect to AI provider |
| `/undo` | - | Undo last action |
| `/redo` | - | Redo undone action |
| `/model` | `/m` | Switch AI model |
| `/clear` | `/new`, `/reset` | Start new conversation |
| `/help` | `/?`, `/commands` | Show available commands |
| `/session` | `/s` | Manage sessions |
| `/export` | - | Export conversation |
| `/config` | - | View/modify config |

## Benefits

1. **Discoverability**: Users can type `/` to see available commands
2. **Efficiency**: Quick access without memorizing shortcuts
3. **Extensibility**: Easy to add new commands
4. **Consistency**: Familiar pattern from Discord, Slack, etc.
5. **Autocomplete**: Tab completion for commands and arguments

## Implementation Effort

| Task | Effort | Priority |
|------|--------|----------|
| Command Registry | 1-2 days | High |
| Built-in Commands | 2-3 days | High |
| TUI Integration | 1-2 days | High |
| Autocomplete | 1 day | Medium |
| Plugin Commands | 1 day | Low |

## Files to Create/Modify

```
libs/shared/src/
├── commands/              # NEW
│   ├── mod.rs
│   ├── registry.rs
│   ├── builtin.rs
│   └── context.rs

tui/src/
├── services/
│   ├── commands.rs        # NEW
│   └── handlers/
│       └── input.rs       # MODIFY
└── app.rs                 # MODIFY: add command_service
```
