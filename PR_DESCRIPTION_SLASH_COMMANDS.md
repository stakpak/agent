# PR: Custom Slash Commands

## Summary

Adds custom slash commands: predefined prompts (e.g. `/code-review`) or user-created from markdown files. Commands appear in the TUI helper dropdown when typing `/` and inject content as the user message.

## Feature Overview

| Type | Location | ID |
|------|----------|-----|
| Predefined | `libs/api/src/slash_commands/*.md` (embedded) | `/name` |
| Personal | `~/.stakpak/commands/cmd_*.md` | `/cmd:name` |
| Project | `./.stakpak/commands/cmd_*.md` | `/cmd:name` |
| Config | `[profiles.default.commands.definitions]` | `/cmd:name` |

Precedence: Config > Project > Personal > Predefined.

## Usage

```bash
# Personal
mkdir -p ~/.stakpak/commands
echo -e "# Deploy\nDeploy to staging..." > ~/.stakpak/commands/cmd_deploy.md

# Project
mkdir -p .stakpak/commands
echo -e "# Test\nRun tests" > .stakpak/commands/cmd_test.md
```

```toml
[profiles.default.commands]
file_prefix = "cmd_"   # optional
id_prefix = "/cmd:"    # optional
include = ["security-*", "write-*"]
exclude = ["*-deprecated"]

[profiles.default.commands.definitions]
security-review = "~/my-prompts/custom-security.md"
```

## Flow

```
1. CLI starts → main loads config → profile.commands → CommandsConfig
   → run_interactive(config) receives commands_config

2. TUI starts → App::new(commands_config)
   → scan_custom_commands(commands_config) → Vec<CustomCommand>
   → get_helper_commands(custom) → builtin + custom → Vec<HelperEntry>
   → FileSearch channel gets commands_config

3. User types "/" → file_search receives input
   → scan_custom_commands() again (dynamic reload)
   → get_helper_commands(custom) → filter by search term → filtered_helpers
   → helper_dropdown shows list

4. User selects custom command (e.g. /cmd:deploy)
   → execute_command("/cmd:deploy", ctx)
   → Find in state.custom_commands → inject cmd.content as UserMessage
   → OutputEvent::UserMessage → agent processes
```

## Code: Files & Functions

| File | Function | Purpose |
|------|----------|---------|
| **libs/shared** | | |
| `path_utils.rs` | `expand_path(path) -> PathBuf` | Expand `~/` to home dir |
| `markdown.rs` | `extract_markdown_title(content) -> Option<String>` | First `# Title` H1 |
| `file_scanner.rs` | `scan_flat_markdown_dir(dir, prefix?, max_bytes) -> Vec<(String,String)>` | Flat dir scan, strip prefix from stem, skip large files |
| `models/commands.rs` | `CustomCommand`, `CommandSource`, `CommandsConfig` | Types |
| | `CommandsConfig::file_prefix()`, `id_prefix()`, `should_load(name)` | Config getters, include/exclude globs |
| **libs/api/slash_commands** | | |
| `predefined.rs` | `PREDEFINED_COMMANDS` (Lazy) | `include_dir!` embeds `*.md` at compile time |
| `scanner.rs` | `scan_commands(config?) -> Vec<CustomCommand>` | Merge predefined → personal → project → definitions |
| | `add_command(by_id, id, name, content, source)` | Extract description, insert into map |
| **tui/services/commands.rs** | `scan_custom_commands(config?)` | Delegates to `stakpak_api::commands::scan_commands` |
| | `get_helper_commands(custom)` | Merge builtin + custom, skip custom IDs that match builtin |
| | `execute_command(id, ctx)` | Custom: inject content as UserMessage; builtin: run handler |
| **tui/services/file_search.rs** | (in loop) | When input starts with `/`, calls scan + get_helper for dynamic reload |
| **cli** | | |
| `config/commands.rs` | — | Re-exports `CommandsConfig` |
| `config/profile.rs` | `commands: Option<CommandsConfig>` | Profile-level commands config |
| `main.rs`, `mode_interactive.rs` | Pass `commands_config` | Forwards config to TUI |

## Files Changed

`git diff main -- libs/shared libs/api tui cli docs`

- **NEW**: `models/commands`, `path_utils`, `markdown`, `file_scanner`, `slash_commands/*`
- **ADD**: `profile.commands`, `commands_config` in run_interactive, TUI scan/execution
- **DOCS**: `docs/architecture-enhancements/05-slash-commands.md` — architecture, files, function logic, user guide (replaces old proposal)
- **REMOVED**: `docs/slash_commands.md`, `docs/slash_commands_architecture.md`

## Testing

```bash
cargo test -p stakpak-shared -- path_utils markdown file_scanner models::commands
cargo test -p stakpak-api -- slash_commands
```

## Docs

- `docs/architecture-enhancements/05-slash-commands.md` — architecture, files, function logic, user guide

## Notes

- `commands_config = None` → defaults; loads all sources
- Definition paths: `~/` expanded; relative paths are CWD-relative
- `readonly_profile` doesn't copy commands — subagents are headless, no TUI
- Matches `RulebookConfig` pattern (include/exclude, `matches_glob`)
