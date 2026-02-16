# Enhancement Proposal: Slash Commands System

## Overview

Custom slash commands let users run predefined prompts (e.g. `/code-review`, `/write-pr`) or create their own from markdown files. Commands appear in the TUI helper dropdown when typing `/` and inject content as the user message. Precedence: Config > Project > Personal > Predefined.

## Command Sources (Precedence)

| Order | Source | Location | ID Format |
|-------|--------|----------|-----------|
| 1 (lowest) | Predefined | `libs/api/src/slash_commands/*.md` (embedded) | `/name` |
| 2 | Personal | `~/.stakpak/commands/{file_prefix}*.md` | `{id_prefix}name` |
| 3 | Project | `./.stakpak/commands/{file_prefix}*.md` | `{id_prefix}name` |
| 4 (highest) | Config | `[profiles.default.commands.definitions]` | `{id_prefix}name` |

## Architecture

### Flow

```
1. CLI starts
   └─► main.rs loads config → profile.commands → CommandsConfig
   └─► run_interactive(config) receives commands_config

2. TUI starts
   └─► App::new() receives commands_config
   └─► scan_custom_commands(commands_config) → Vec<CustomCommand>
   └─► get_helper_commands(custom) → builtin + custom → Vec<HelperEntry>
   └─► FileSearch channel gets commands_config for later use

3. User types "/"
   └─► file_search receives input
   └─► If input.starts_with('/'): scan_custom_commands() again (dynamic reload)
   └─► get_helper_commands(custom) → fresh helpers
   └─► Filter by search term → filtered_helpers
   └─► helper_dropdown shows list

4. User selects custom command (e.g. /cmd:deploy)
   └─► execute_command("/cmd:deploy", ctx)
   └─► Find in state.custom_commands → inject cmd.content as UserMessage
   └─► Send OutputEvent::UserMessage → agent processes
```

### Data Flow (scan path)

```
User types "/" in TUI
       │
       ▼
file_search.rs: scan_custom_commands(commands_config)
       │
       ▼
stakpak_api::slash_commands::scan_commands(config)
       │
       ├─► PREDEFINED_COMMANDS (include_dir, embedded *.md)
       ├─► ~/.stakpak/commands/ via file_scanner::scan_flat_markdown_dir
       ├─► ./.stakpak/commands/ via file_scanner::scan_flat_markdown_dir
       └─► config.definitions (path_utils::expand_path for ~/)
       │
       ▼
Vec<CustomCommand> merged by id (later overwrites earlier)
       │
       ▼
get_helper_commands(custom) → builtin + custom → HelperEntry list
       │
       ▼
helper_dropdown renders; execute_command runs selected command
```

### Key Components

| Component | File | Purpose |
|-----------|------|---------|
| `CustomCommand` | `libs/shared/src/models/commands.rs` | Loaded command: id, description, content, source |
| `CommandsConfig` | `libs/shared/src/models/commands.rs` | Config: file_prefix, id_prefix, include, exclude, definitions |
| `CommandSource` | `libs/shared/src/models/commands.rs` | Predefined, PersonalFile, ProjectFile, ConfigDefinition |
| `CommandsConfig::file_prefix()`, `id_prefix()`, `should_load(name)` | `libs/shared/src/models/commands.rs` | Config getters; include/exclude via `matches_glob` |
| `expand_path(path) -> PathBuf` | `libs/shared/src/path_utils.rs` | Expand `~/` to home dir |
| `extract_markdown_title(content) -> Option<String>` | `libs/shared/src/markdown.rs` | First `# Title` H1 → description |
| `scan_flat_markdown_dir(dir, prefix?, max_bytes) -> Vec<(String,String)>` | `libs/shared/src/file_scanner.rs` | Flat dir scan, strip prefix from stem, skip files > max_bytes |
| `PREDEFINED_COMMANDS` (Lazy) | `libs/api/src/slash_commands/predefined.rs` | `include_dir!` embeds `*.md` at compile time |
| `scan_commands(config?) -> Vec<CustomCommand>` | `libs/api/src/slash_commands/scanner.rs` | Merge predefined → personal → project → definitions |
| `add_command(by_id, id, name, content, source)` | `libs/api/src/slash_commands/scanner.rs` | Extract description, insert into map |
| `scan_custom_commands(config?)` | `tui/src/services/commands.rs` | Delegates to `stakpak_api::commands::scan_commands` |
| `get_helper_commands(custom)` | `tui/src/services/commands.rs` | Merge builtin + custom; skip custom IDs that match builtin |
| `execute_command(id, ctx)` | `tui/src/services/commands.rs` | Custom: inject content as UserMessage; builtin: run handler |
| (file_search loop) | `tui/src/services/file_search.rs` | When input starts with `/`, calls scan + get_helper for dynamic reload |

## Function Logic

### scan_commands (scanner.rs)

1. Resolve `file_prefix` and `id_prefix` from config or defaults (`cmd_`, `/cmd:`)
2. Load predefined: iterate `PREDEFINED_COMMANDS`, apply `should_load`, insert with id `/name`
3. Load personal: `~/.stakpak/commands/` via `scan_flat_markdown_dir`, insert with `{id_prefix}{name}`
4. Load project: `./.stakpak/commands/` same way (overwrites personal by id)
5. Load definitions: for each `config.definitions` entry, `expand_path` file, read content, insert
6. Sort by id, return `Vec<CustomCommand>`

### add_command (scanner.rs)

`description` = `extract_markdown_title(content)` or fallback to `name`. Insert into `by_id` HashMap (later insert overwrites).

### CommandsConfig::should_load (commands.rs)

`matches_include(name) && matches_exclude(name)`. Uses `crate::utils::matches_glob` for include/exclude patterns.
- `include: none` → allow all
- `include: ["security-*"]` → only names matching
- `exclude: ["*-deprecated"]` → exclude if any match
- Exclude wins over include

## Files to Create/Modify

```
libs/shared/src/
├── models/
│   └── commands.rs      # NEW: CustomCommand, CommandsConfig, CommandSource
├── path_utils.rs        # NEW: expand_path
├── markdown.rs          # NEW: extract_markdown_title
├── file_scanner.rs      # NEW: scan_flat_markdown_dir
└── utils.rs             # ADD: matches_glob (used by CommandsConfig)

libs/api/src/
├── slash_commands/      # NEW
│   ├── mod.rs
│   ├── predefined.rs    # include_dir, PREDEFINED_COMMANDS
│   ├── scanner.rs       # scan_commands
│   ├── code-review.md   # predefined prompt
│   ├── write-pr.md
│   └── write-rfc.md

cli/src/
├── config/
│   ├── commands.rs      # NEW: re-export CommandsConfig
│   └── profile.rs       # ADD: commands in ProfileConfig
└── commands/agent/run/
    └── mode_interactive.rs  # ADD: commands_config

tui/src/
├── app.rs               # ADD: custom_commands, commands_config
├── app/types.rs         # ADD: CustomCommand re-export
├── event_loop.rs        # ADD: commands_config
├── services/
│   ├── commands.rs      # ADD: scan_custom_commands, get_helper_commands
│   ├── file_search.rs   # ADD: scan on slash trigger
│   └── helper_dropdown.rs  # (unchanged, uses helpers)
```

## Configuration

In `[profiles.default.commands]`:

- `file_prefix` (default `"cmd_"`) — only `{file_prefix}*.md` loaded from dirs; use `""` to load any `*.md`
- `id_prefix` (default `"/cmd:"`) — user commands become `{id_prefix}name`
- `include` / `exclude` — glob patterns to filter commands

```toml
[profiles.default.commands]
file_prefix = "cmd_"
id_prefix = "/cmd:"
include = ["security-*", "write-*"]
exclude = ["*-deprecated"]

[profiles.default.commands.definitions]
security-review = "~/my-prompts/custom-security.md"
```

## How to Add Commands

### Predefined (Embedded)

**Location:** `libs/api/src/slash_commands/*.md`

Add a new `.md` file (e.g. `my-command.md`) → it becomes `/my-command` automatically.

### Personal Commands

**Location:** `~/.stakpak/commands/cmd_*.md`

```bash
mkdir -p ~/.stakpak/commands
echo -e "# Deploy\nDeploy to staging..." > ~/.stakpak/commands/cmd_deploy.md
# Available as /cmd:deploy
```

### Project Commands

**Location:** `./.stakpak/commands/cmd_*.md`

Project-specific commands override personal commands with the same name.

### Config Definitions (Highest Priority)

Override any command with explicit file paths in `[profiles.default.commands.definitions]`. Paths support `~/` expansion; relative paths are CWD-relative.

## Implementation Notes

**Existing scanners not reused:** `file_watcher`, `file_search`, `code_index` serve different domains (recursive walk, file suggestions, indexing). Commands need flat dir scan returning `(name, content)`.

**Deferred:** `CommandSource` trait for remote/cache sources.
