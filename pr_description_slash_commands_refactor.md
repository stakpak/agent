# PR: Refactor Slash Commands Architecture

## Summary

Refactors the custom slash commands system to eliminate code duplication and centralize shared types. This creates a cleaner architecture that's ready for future remote-updateable predefined commands.

## Problem

The original implementation had several architectural issues:
- `CommandsConfig` was duplicated in CLI (`cli/src/config/commands.rs`) and TUI (`tui/src/event_loop.rs`)
- `CustomCommand` type was TUI-only, couldn't be used elsewhere
- `matches_pattern()` glob matching was triplicated across files
- `scan_custom_commands()` domain logic lived in TUI crate instead of API crate
- Manual field-by-field conversion between CLI and TUI config types

## Solution

### 1. Centralized Types in `libs/shared`

**New file: `libs/shared/src/models/commands.rs`**
```rust
pub struct CustomCommand {
    pub id: String,
    pub description: String,
    pub content: String,
    pub source: CommandSource,  // NEW: track where command came from
}

pub enum CommandSource {
    Predefined,        // Embedded in binary
    PredefinedRemote,  // Fetched from API (future)
    PersonalFile,      // ~/.stakpak/commands/cmd_*.md
    ProjectFile,       // ./.stakpak/commands/cmd_*.md
    ConfigDefinition,  // Explicit file path in config
}

pub struct CommandsConfig {
    pub include: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub definitions: HashMap<String, String>,
}
```

**New function: `libs/shared/src/utils.rs`**
```rust
pub fn matches_glob(value: &str, pattern: &str) -> bool
```

### 2. Moved Scanning Logic to `libs/api`

**Renamed: `predefined_commands/` → `slash_commands/`**

New structure:
```
libs/api/src/slash_commands/
├── mod.rs           # Module exports
├── predefined.rs    # PREDEFINED_COMMANDS constant
├── scanner.rs       # scan_commands() function (moved from TUI)
├── code-review.md
├── explain.md
├── quick-fix.md
├── security-review.md
└── write-tests.md
```

### 3. Updated Consumers

- **CLI**: `cli/src/config/commands.rs` now re-exports from shared
- **CLI**: `cli/src/config/rulebook.rs` uses shared `matches_glob()`
- **TUI**: `tui/src/event_loop.rs` removed duplicate `CommandsConfig` (51 lines)
- **TUI**: `tui/src/app/types.rs` re-exports `CustomCommand` from shared
- **TUI**: `tui/src/services/commands.rs` delegates to `stakpak_api::commands::scan_commands()`
- **CLI→TUI**: No more manual config conversion, same type used directly

## Command Sources & How to Update

### Predefined Commands (Embedded)
**Location:** `libs/api/src/slash_commands/*.md`

To update predefined commands:
1. Edit the `.md` files in `libs/api/src/slash_commands/`
2. Add new commands to `PREDEFINED_COMMANDS` in `predefined.rs`
3. Rebuild and release

### Personal Commands
**Location:** `~/.stakpak/commands/cmd_*.md`

Users can create personal commands that work across all projects:
```bash
mkdir -p ~/.stakpak/commands
echo "# My Command\nDo something..." > ~/.stakpak/commands/cmd_mycommand.md
# Available as /cmd:mycommand
```

### Project Commands
**Location:** `./.stakpak/commands/cmd_*.md`

Project-specific commands that override personal commands:
```bash
mkdir -p .stakpak/commands
echo "# Deploy\nDeploy to staging..." > .stakpak/commands/cmd_deploy.md
# Available as /cmd:deploy
```

### Config Definitions (Highest Priority)
**Location:** `~/.stakpak/config.toml`

Override any command with explicit file paths:
```toml
[profiles.default.commands.definitions]
security-review = "~/my-prompts/custom-security.md"
```

## File Changes

| File | Change |
|------|--------|
| `libs/shared/src/models/commands.rs` | **NEW** - Shared types |
| `libs/shared/src/utils.rs` | **ADD** - `matches_glob()` function |
| `libs/api/src/slash_commands/` | **NEW** - Renamed from `predefined_commands/` |
| `libs/api/src/slash_commands/scanner.rs` | **NEW** - Moved from TUI |
| `cli/src/config/commands.rs` | **SIMPLIFIED** - Re-export only (51→5 lines) |
| `cli/src/config/rulebook.rs` | **SIMPLIFIED** - Use shared glob |
| `tui/src/event_loop.rs` | **SIMPLIFIED** - Remove duplicate (51 lines removed) |
| `tui/src/app/types.rs` | **SIMPLIFIED** - Re-export from shared |
| `tui/src/services/commands.rs` | **SIMPLIFIED** - Delegate to API (185 lines removed) |
| `cli/src/commands/agent/run/mode_interactive.rs` | **SIMPLIFIED** - Direct config pass |

## Stats

```
21 files changed, 506 insertions(+), 299 deletions(-)
```

Net: ~200 lines of duplicated code removed, cleaner separation of concerns.

## Future: Remote-Updateable Predefined Commands

The architecture now supports adding remote-fetched commands without a release:

1. Add `PredefinedCommandsCache` in `libs/api/src/slash_commands/cache.rs`
2. Fetch from `GET /v1/predefined-commands` on startup
3. Cache to `~/.stakpak/cache/predefined_commands.json`
4. Fall back to embedded `PREDEFINED_COMMANDS` when offline

Non-devs can update commands via Stakpak dashboard → API → users get fresh commands on next startup.

## Testing

```bash
cargo check                    # ✓ Compiles
cargo test --workspace         # ✓ All tests pass
cargo clippy --all-targets     # ✓ No warnings
```
