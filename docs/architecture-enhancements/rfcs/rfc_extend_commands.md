# RFC: User-Extended Slash Commands

> **Status**: Draft  
> **Author**: Stakpak Team  
> **Related**: [extend_commands.md](./extend_commands.md) (competitive analysis)

---

## 1. Summary

Enable users to define custom slash commands via **markdown files on disk**. A command like `/create-component` would be populated from a file; the file content is injected as the user message sent to the LLM.

**User flow:**
1. User creates `commands/create-component.md` (or `~/.stakpak/commands/create-component.md`)
2. User types `/create-component` in the TUI
3. Stakpak loads the file, optionally substitutes `$ARGUMENTS`, and sends the content as `OutputEvent::UserMessage`
4. The agent processes it like any other user message

---

## 2. Architectural Options

### Option A: Minimal (Easiest)

**Discovery:** One flat directory. Filename (without `.md`) → command ID.

| Location | Purpose |
|----------|---------|
| `.stakpak/commands/*.md` | Project commands |
| `~/.stakpak/commands/*.md` | Personal (global) commands |

**Mapping:**
- `create-component.md` → `/create-component`
- `code-review.md` → `/code-review`

**Arguments:** None in v1. `$ARGUMENTS` placeholder supported later.

**Execution:** Load file content, send as `UserMessage`. No frontmatter.

**Pros:** Trivial to implement. One `read_dir` + filter `.md`, one HashMap lookup.  
**Cons:** No namespacing, no args, no metadata.

---

### Option B: With Subdirectories (Namespace Support)

**Discovery:** Recursive scan. Path relative to commands dir → command ID with `/` separator.

| Path | Command ID |
|------|------------|
| `commands/create-component.md` | `/create-component` |
| `commands/frontend/component.md` | `/frontend/component` |
| `commands/deploy/k8s.md` | `/deploy/k8s` |

**Mapping rules:**
- Strip `.md` from filename
- Join path segments with `/`
- Leading `/` implied

**Pros:** Organize commands (`/deploy/k8s`, `/frontend/component`).  
**Cons:** Slightly more logic; need recursive walk.

---

### Option C: With YAML Frontmatter

**File format:**
```markdown
---
description: "Generate a React component"
argument-hint: "[component-name]"
---
Create a new React component. Follow our style guide.
If component name is provided, use it; otherwise suggest one.
```

**Parsing:** Use existing frontmatter helpers (rulebooks use this). `description` for autocomplete; `argument-hint` for future UX.

**Pros:** Rich metadata, aligns with Claude/OpenCode.  
**Cons:** Parsing overhead; v1 could skip and add later.

---

### Option D: Argument Substitution

**Placeholders:**
- `$ARGUMENTS` — rest of input after command (e.g. `/review src/lib.rs` → `$ARGUMENTS` = `src/lib.rs`)
- Future: `$1`, `$2` positional; `$NAME` named (OpenCode-style)

**Example file:**
```markdown
Review the file $ARGUMENTS for security issues.
Focus on input validation and authentication.
```

**Execution:** Split user input on first space; command = before space, args = after. Replace `$ARGUMENTS` in template.

**Pros:** One prompt, many uses.  
**Cons:** Requires parsing input; edge cases (quotes, spaces).

---

## 3. Recommended Phasing

### Phase 1: Option A (Minimal)

- **Scope:** Flat `.stakpak/commands/*.md` + `~/.stakpak/commands/*.md`
- **Behavior:** File content = user message. No args, no frontmatter.
- **Effort:** ~1–2 days
- **Deliverables:** Scan at TUI init, merge with built-in commands, execute via `UserMessage`

### Phase 2: Option B (Subdirectories)

- **Scope:** Recursive scan, path → command ID
- **Effort:** ~0.5 day
- **Depends on:** Phase 1

### Phase 3: Option C (Frontmatter)

- **Scope:** `description` for autocomplete, optionally `argument-hint`
- **Effort:** ~1 day
- **Depends on:** Phase 1

### Phase 4: Option D (Arguments)

- **Scope:** `$ARGUMENTS` substitution
- **Effort:** ~1 day
- **Depends on:** Phase 1

---

## 4. Implementation Plan (Phase 1)

### 4.1 Data Model

```rust
// New type for custom commands (or extend HelperCommand)
pub struct CustomCommand {
    pub id: String,           // e.g. "/create-component"
    pub description: String,  // from filename or "Custom: create-component"
    pub path: PathBuf,       // path to .md file (for loading at execution time)
}

// Merge strategy: built-in + custom
pub fn get_helper_commands() -> Vec<HelperCommand> {
    let builtin = commands_to_helper_commands();
    let custom = scan_custom_commands(); // new
    merge_commands(builtin, custom)      // built-in first, custom appended
}
```

**Note:** `HelperCommand` uses `&'static str`. For custom commands we need owned `String`. Options:
1. Add `HelperCommand::Custom { id: String, description: String }` variant
2. Or introduce `HelperEntry { id: Cow<str>, description: Cow<str>, source: Builtin | Custom(path) }`

Simplest: a parallel `custom_commands: Vec<CustomCommand>` and merge at the point of display/execution.

### 4.2 Discovery

```rust
fn scan_custom_commands() -> Vec<CustomCommand> {
    let mut commands = Vec::new();
    for dir in [".stakpak/commands", "~/.stakpak/commands"] {
        let path = expand_path(dir);
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().map_or(false, |e| e == "md") {
                    let id = format!("/{}", p.file_stem().unwrap().to_string_lossy());
                    let desc = format!("Custom: {}", id.trim_start_matches('/'));
                    commands.push(CustomCommand { id, description: desc, path: p });
                }
            }
        }
    }
    commands
}
```

**Priority:** Project overrides personal (or merge both; first match wins). Document behavior.

### 4.3 Execution Flow

In `execute_command`:

```rust
pub fn execute_command(command_id: CommandId, ctx: CommandContext) -> Result<(), String> {
    // 1. Check built-in first
    match command_id {
        "/help" => { ... }
        "/init" => { ... }
        // ... all built-ins
        _ => {}
    }

    // 2. Fallback: custom command
    if let Some(cmd) = ctx.state.custom_commands.iter().find(|c| c.id == command_id) {
        let content = tokio::fs::read_to_string(&cmd.path).await
            .map_err(|e| format!("Failed to load command: {}", e))?;
        let prompt = content.trim().to_string();
        if !prompt.is_empty() {
            ctx.state.messages.push(Message::info(prompt.clone(), ...));
            let _ = ctx.output_tx.try_send(OutputEvent::UserMessage(prompt, ...));
        }
        ctx.state.text_area.set_text("");
        ctx.state.show_helper_dropdown = false;
        return Ok(());
    }

    Err(format!("Unknown command: {}", command_id))
}
```

**Note:** `execute_command` is sync in the current codebase. File read is async. Options:
1. Use `tokio::fs::read_to_string` in an async context (if caller is async)
2. Or load all custom command contents at init and cache (simpler, small memory cost)

Caching at init is easier and keeps `execute_command` sync.

### 4.4 Integration Points

| File | Change |
|------|--------|
| `tui/src/app.rs` | Add `custom_commands: Vec<CustomCommand>` to `AppState`; merge into helpers for display |
| `tui/src/services/commands.rs` | Add `scan_custom_commands()`, `merge_commands()`; extend `execute_command` fallback |
| `tui/src/services/file_search.rs` | Pass merged helpers (built-in + custom) to worker; filtering works if custom use same shape |
| `tui/src/app/types.rs` | Extend `HelperCommand` or add `HelperEntry` enum for dynamic commands |

### 4.5 HelperCommand Compatibility

Current: `HelperCommand { command: &'static str, description: &'static str }`.

To support custom commands in the dropdown, we need dynamic strings. Options:

1. **Add variant:** `enum HelperEntry { Builtin(HelperCommand), Custom { id: String, description: String } }`
2. **Use owned type:** Change `HelperCommand` to `{ command: String, description: String }` and make built-ins use `.into()` or lazy_static.

Option 1 is backward-compatible and explicit. File search filters by `command.contains(input)`; both variants can support that.

---

## 5. Directory Layout (User-Facing)

```
# Project commands (version-controlled)
.stakpak/
└── commands/
    ├── create-component.md
    ├── code-review.md
    └── deploy/
        └── k8s.md          # Phase 2: /deploy/k8s

# Personal commands (user-wide)
~/.stakpak/
└── commands/
    ├── explain.md
    └── refactor.md
```

---

## 6. Example Custom Command

**File:** `.stakpak/commands/create-component.md`

```markdown
Create a new React functional component following our project conventions:
- Use TypeScript
- Use hooks (useState, useEffect as needed)
- Export as default
- Include JSDoc for props
- Place in src/components/
```

**Usage:** User types `/create-component` and Enter. Content is sent as the user message.

---

## 7. Open Questions

1. **Priority:** Project vs personal when same command exists in both?
2. **Hot reload:** Rescan on `/commands` or when project dir changes? (v1: init only)
3. **Size limit:** Cap file size (e.g. 64KB) like init.md?
4. **Override built-in:** Allow custom `/help` to override? (Recommend: no in v1; built-in wins)

---

## 8. Acceptance Criteria (Phase 1)

- [ ] User can create `.stakpak/commands/<name>.md` and `~/.stakpak/commands/<name>.md`
- [ ] Commands appear in autocomplete when typing `/`
- [ ] Selecting a custom command injects file content as user message
- [ ] Built-in commands take precedence over custom (no override)
- [ ] File size capped (e.g. 64KB) to prevent abuse
- [ ] `cargo fmt`, `cargo clippy`, `cargo test` pass

---

## 9. References

- [extend_commands.md](./extend_commands.md) — Competitive analysis
- [rfc_stakpak_init.md](./rfc_stakpak_init.md) — `/init` command implementation
- [05-slash-commands.md](./architecture-enhancements/05-slash-commands.md) — Slash command architecture proposal
