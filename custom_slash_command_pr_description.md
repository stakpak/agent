# PR: Custom slash commands with `/cmd:` prefix and global allowlist

## Summary

User-defined slash commands are loaded from markdown files (`cmd_*.md`) in project and personal command directories. They appear in the TUI with a **`/cmd:`** prefix (e.g. `/cmd:write-rfc`, `/cmd:create-component`) so they are clearly distinct from built-in commands. An optional **global config allowlist** limits which custom commands are loaded at startup and when the user types `/`.

---

## Workflows

### 1. Default: load all custom commands

**No config**  
- User has `.stakpak/commands/cmd_write-rfc.md` and `~/.stakpak/commands/cmd_deploy-staging.md`.
- On TUI start and whenever input starts with `/`, the app scans both directories.
- All discovered `cmd_*.md` files become slash commands.
- Dropdown shows built-ins (`/help`, `/clear`, …) plus `/cmd:write-rfc`, `/cmd:deploy-staging`.

**Flow:**
1. `run_tui(..., custom_commands_allowlist: None)`.
2. `AppState::new` calls `scan_custom_commands(None)` → returns every valid custom command.
3. `get_helper_commands(&custom_commands)` merges built-ins + custom; custom entries use id and display = `/cmd:{name}`.
4. User types `/` → file search worker runs `scan_custom_commands(None)` again and filters helpers by query; result includes refreshed `custom_commands` so the UI stays in sync.

---

### 2. Global allowlist: load only listed commands

**Config (e.g. `~/.stakpak/config.toml`):**
```toml
[settings]
custom_commands = ["write-rfc", "create-component"]
```

**Behavior:**
- Only commands whose **name** (the part after `cmd_`) is in `custom_commands` are loaded.
- Other `cmd_*.md` files in the same directories are ignored.

**Flow:**
1. CLI loads `AppConfig`; `config.custom_commands` = `Some(["write-rfc", "create-component"])`.
2. `main.rs` passes `custom_commands` into `RunInteractiveConfig` and then into `run_tui(..., custom_commands_allowlist: Some(list))`.
3. `AppState::new(AppStateOptions { custom_commands_allowlist: Some(list), ... })` calls `scan_custom_commands(Some(&list))`; only matching names are kept.
4. File search worker is spawned with the same `custom_commands_allowlist`; on each `/` it calls `scan_custom_commands(allowlist.as_deref())`, so the same filtering applies on dynamic reload.

**Use case:** Restrict which custom commands are visible (e.g. team vs personal, or only a few heavy prompts).

---

### 3. Project vs personal precedence

**Locations:**
- **Personal:** `~/.stakpak/commands/`
- **Project:** `{cwd}/.stakpak/commands/`

**Scan order:** Personal is read first, then project. For a given command **name**, the project file overwrites the personal one in the in-memory map.

**Example:**
- `~/.stakpak/commands/cmd_write-rfc.md` → "My personal RFC prompt"
- `./.stakpak/commands/cmd_write-rfc.md` → "Team RFC template"
- Result: only the project version is used; id = `/cmd:write-rfc`, content = team template.

---

### 4. Invoking a custom command

**From dropdown:**
1. User types `/` (or `/cmd:` or partial name).
2. File search worker runs `scan_custom_commands(allowlist)`, `get_helper_commands(&custom)`, filters by query.
3. User selects e.g. `/cmd:write-rfc`.
4. Handler gets `selected_helper.command()` = `/cmd:write-rfc`.
5. `execute_command("/cmd:write-rfc", ctx)` runs: looks up `ctx.state.custom_commands` by id, finds the command, pushes `Message::user(content)`, sends `OutputEvent::UserMessage(content, ...)`.
6. Backend treats it as a normal user message (same path as typed input).

**From typing:**  
If the user types `/cmd:write-rfc` and submits, the same execution path runs (helper selection or direct submit that matches a custom command id).

---

### 5. Creating a new custom command (file-based)

**Convention:** `cmd_{name}.md` → slash command `/cmd:{name}`.

**Example:**
- Add `.stakpak/commands/cmd_security-review.md` with content (first `# Title` line becomes description in the dropdown).
- No restart: on next `/`, the file search worker rescans and the new command appears in the list.

**Limits:**  
- File size ≤ 64KB.  
- `.md` only; must start with `cmd_`; name after prefix non-empty.

---

## Implementation details

### Naming and constants

| Concept | Value | Where |
|--------|--------|--------|
| File prefix (on disk) | `cmd_` | `CMD_FILE_PREFIX` in `tui/src/services/commands.rs` |
| Slash prefix (TUI id and display) | `/cmd:` | `CMD_PREFIX` in same file |
| Example | `cmd_write-rfc.md` → `/cmd:write-rfc` | Id and display both `/cmd:write-rfc` |

Built-in commands keep their own ids (`/help`, `/clear`, etc.); only user-defined ones use `/cmd:`.

### Data flow (high level)

```
config.toml [settings].custom_commands (optional)
    → AppConfig.custom_commands
    → main.rs: custom_commands cloned into RunInteractiveConfig
    → mode_interactive: custom_commands_for_tui passed into run_tui
    → run_tui(..., custom_commands_allowlist)
    → AppStateOptions.custom_commands_allowlist
    → AppState::new: scan_custom_commands(allowlist.as_deref()) → custom_commands
    → get_helper_commands(&custom_commands) → helpers (built-in + /cmd: entries)
    → init_file_search_channels(helpers, allowlist) → worker receives allowlist
On each "/" input:
    → file_search_worker: scan_custom_commands(allowlist.as_deref()) → fresh custom list
    → get_helper_commands(&custom) → filtered_helpers
    → Result to UI; state.custom_commands updated so execute_command can resolve by id
```

### Key types

- **`CustomCommand`** (TUI): `id` (e.g. `/cmd:write-rfc`), `description`, `content` (full markdown).
- **`HelperEntry`**: either `Builtin(HelperCommand)` or `Custom { command, display, description }`. For custom, `command` and `display` are both `/cmd:{name}`.
- **`scan_custom_commands(allowlist)`**: returns `Vec<CustomCommand>`. If `allowlist` is `Some`, only names in the list are included.

### Execution path

- **`execute_command(command_id, ctx)`** in `tui/src/services/commands.rs`:
  - First checks `ctx.state.custom_commands` for an entry with `c.id == command_id` (e.g. `/cmd:write-rfc`).
  - If found: push user message with `cmd.content`, send `OutputEvent::UserMessage`, clear input, invalidate message cache, return `Ok(())`.
  - Else: fall through to built-in `match command_id { "/help" => ..., ... }`.

### Config (global)

- **`Settings.custom_commands`**: `Option<Vec<String>>`. Command **names** only (e.g. `["write-rfc", "create-component"]`), not paths or ids.
- **`AppConfig.custom_commands`**: same, from `Settings` when loading config.
- **TOML:** Under `[settings]`, key `custom_commands = ["name1", "name2"]`. If omitted or empty, allowlist is `None` and all discovered custom commands are loaded.

---

## Files changed (summary)

| Area | File | Change |
|------|------|--------|
| **TUI commands** | `tui/src/services/commands.rs` | `CMD_PREFIX`; `scan_custom_commands(allowlist)`; custom id/display = `/cmd:name`; execute custom by id before built-in match; duplicate `/init` removed. |
| **TUI app** | `tui/src/app.rs` | `AppStateOptions.custom_commands_allowlist`; `AppState::new` uses allowlist in `scan_custom_commands` and `init_file_search_channels`. |
| **TUI event loop** | `tui/src/event_loop.rs` | `run_tui(..., custom_commands_allowlist)`; pass into `AppStateOptions`. |
| **TUI file search** | `tui/src/services/file_search.rs` | `file_search_worker(..., custom_commands_allowlist)`; call `scan_custom_commands(allowlist.as_deref())` when filtering on `/`. |
| **CLI config** | `cli/src/config/types.rs` | `Settings.custom_commands`. |
| **CLI config** | `cli/src/config/app.rs` | `AppConfig.custom_commands`; read/write in `build` and `From` impls. |
| **CLI config** | `cli/src/config/file.rs` | Defaults and `set_app_config_settings` for `custom_commands`. |
| **CLI main** | `cli/src/main.rs` | Extract `custom_commands` before `run_interactive`; pass into `RunInteractiveConfig`. |
| **CLI interactive** | `cli/src/commands/agent/run/mode_interactive.rs` | `RunInteractiveConfig.custom_commands`; clone for TUI task; pass into `run_tui`. |
| **Tests** | `tui/src/services/handlers/mod.rs` | `AppStateOptions` test fixture: `custom_commands_allowlist: None`. |

---

## Manual testing suggestions

1. **No allowlist:** Ensure `[settings]` has no `custom_commands`. Add `cmd_foo.md` in `.stakpak/commands/`, start TUI, type `/` → `/cmd:foo` appears; run it → content sent as user message.
2. **With allowlist:** Set `custom_commands = ["foo"]`; only `cmd_foo.md` appears; other `cmd_*.md` files are ignored.
3. **Project overrides personal:** Same name in both directories → project content wins.
4. **Dynamic reload:** Add a new `cmd_*.md` while TUI is open; on next `/` it appears without restart.
5. **Id and display:** Confirm dropdown and execution use `/cmd:...` (not `/...` alone) for custom commands.

---

## Migration (if upgrading from Usercmd_*)

If you had custom command files named `Usercmd_{name}.md`, rename them to `cmd_{name}.md` so they are discovered (e.g. `Usercmd_write-rfc.md` → `cmd_write-rfc.md`). The slash command remains `/cmd:{name}`.

---

## Type of change

- [x] New feature (non-breaking)
- [ ] Breaking change (file naming: Usercmd_* → cmd_*)
- [ ] Bug fix
- [ ] Documentation only
