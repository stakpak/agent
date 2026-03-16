# Hierarchical Shell Command Approval — Feature Specification

## Overview

Shell commands executed by the agent currently pass through a flat tool-name lookup. This feature adds a **scope resolution layer** that parses shell command strings and maps them to hierarchical scope keys stored in the **existing** `tools`, no parallel policy system. A key like `"run_command::rm::-rf"` lives in the same HashMap as `"run_command"` and is simply looked up first.

**Scope:** Interactive TUI path only. Headless/autopilot integration is out of scope.

---

## 1. Motivation

`ToolApprovalPolicy::action_for()` (`libs/agent-core/src/types.rs:164`) resolves tool actions via a flat `HashMap<String, ToolApprovalAction>`. A single entry for `"run_command"` cannot differentiate between:

- `echo "hello"` — harmless, should auto-approve
- `rm -rf /` — destructive, should hard-reject
- `curl --upload-file secret.txt` — sensitive, should escalate

Additionally, `AutoApproveManager` (`tui/src/services/auto_approve.rs`) hardcodes `run_command` to `Prompt` in several places, and the existing `CommandPatterns` struct is defined but never evaluated.

This feature adds scope-aware resolution as an **early-exit layer** before the flat lookup in both `agent-core` and the TUI's `AutoApproveManager`. `CommandPatterns` remains as dead code and can be removed in a follow-up.

---

## 2. New Crate: `libs/shell-tool-approvals`

A new workspace crate handles all parsing and scope resolution. It has **no dependencies** on `agent-core`, `tui`, or any other workspace crate — it is a pure library.

### 2.1 Public API

```rust
// libs/shell-tool-approvals/src/lib.rs
pub use parse::{ParsedCommand, parse};
```

### 2.2 Dependencies

| Crate              | Purpose                       |
|--------------------|-------------------------------|
| `tree-sitter`      | Incremental parsing framework |
| `tree-sitter-bash` | Bash grammar                  |
| `globset`          | Glob pattern matching         |
| `regex`            | Regex pattern matching        |

---

## 3. Parsing

### 3.1 Tree-sitter AST

Every `run_command` / `run_command_task` command string is parsed with `tree-sitter-bash`. The parser performs a depth-first traversal and collects every `command` node into a flat list.

| Node Type              | Handling                                            |
|------------------------|-----------------------------------------------------|
| `command`              | Extract command name and arguments                  |
| `pipeline`             | Recurse into each piped command                     |
| `command_substitution` | Recurse into `$(...)` / backtick body               |
| `list` / `compound`   | Recurse into `&&`, `||`, `;` sequences              |
| `subshell`             | Recurse into `(...)` body                           |
| `variable_expansion`   | Preserved as a literal `$VAR` — never resolved      |

Node types not listed above (heredocs, redirections, etc.) are ignored for scope resolution but do not cause parse failures.

### 3.2 Output: `ParsedCommand`

```rust
/// A single command extracted from a shell script.
pub struct ParsedCommand {
    /// The command name (e.g., "rm", "curl", "git").
    /// None if the command is a bare variable expansion like `$CMD`.
    pub name: Option<String>,
    /// Arguments as they appear in the AST, in order.
    pub args: Vec<String>,
    /// Byte offset in the original input where this command starts.
    pub offset: usize,
}
```

`parse(input: &str) -> Vec<ParsedCommand>` is the single public entry point.

### 3.3 Nested Script Detection

When a shell invocation embeds another script, the inner script is extracted and re-parsed. Commands from the child AST are appended to the parent list.

| Pattern                   | Extraction                           |
|---------------------------|--------------------------------------|
| `sh -c "..."`            | Inner string after `-c`              |
| `bash -c "..."`          | Inner string after `-c`              |
| `env [...] bash -c "..."` | Skip env flags, extract inner string |
| `xargs sh -c "..."`      | Inner string after `-c`              |

Recognised shells: `sh`, `bash`, `zsh`, `fish`, `dash`, `ksh`, `tcsh`, `csh`.

---

## 4. Scope Resolution

### 4.1 Scope Key Structure

Each `ParsedCommand` maps to a chain of scope keys, checked from most specific to least specific:

```
run_command::<command>::<argument-pattern>   // level 3: argument-specific
run_command::<command>                       // level 2: command-level
run_command                                  // level 1: global fallback
```

Examples:

| Command         | Scope chain (most → least specific)                            |
|-----------------|----------------------------------------------------------------|
| `rm -rf /tmp`   | `run_command::rm::-rf` → `run_command::rm` → `run_command`    |
| `curl -X POST`  | `run_command::curl::-X` → `run_command::curl` → `run_command` |
| `ls`            | `run_command::ls` → `run_command`                              |
| `$UNKNOWN_CMD`  | `run_command` (name is None; only the global fallback applies) |

### 4.2 Argument Pattern Matching

Level-3 scope keys support three matching modes against the command's argument list:

| Mode  | Syntax         | Example           | Matches                        |
|-------|----------------|-------------------|--------------------------------|
| Exact | literal string | `-rf`             | Arg exactly equals `-rf`       |
| Glob  | glob pattern   | `/etc/*`          | Arg matching `/etc/foo`        |
| Regex | `re:` prefix   | `re:--output=.*`  | Arg matching `--output=x`      |

A level-3 rule matches if **any** argument in the command's arg list satisfies the pattern. Multiple level-3 rules for the same command are stored as separate HashMap keys and are each looked up independently.

### 4.3 Specificity and Aggregation

Resolution happens in two stages:

1. **Within one scope** (`run_command`, `run_command_task`, etc.), the **most specific** matching rule wins:
   - `scope::<command>::<argument-pattern>`
   - `scope::<command>`
   - `scope`
2. **Across multiple parsed commands** in a pipeline / list / nested script, the **most restrictive** action wins.

This allows rules like `run_command::git::status = approve` to relax the default for that specific command while still ensuring that a later `rm`/`push` in the same shell string escalates the final decision.

Restrictiveness order (implemented via `Ord` on the enum with explicit discriminants):

```
// ToolApprovalAction
Approve = 0  <  Ask = 1  <  Deny = 2

// AutoApprovePolicy
Auto = 0  <  Prompt = 1  <  Never = 2
```

Both enums now derive `PartialOrd` and `Ord`, enabling `.max()` aggregation directly.

---

## 5. Integration Points

### 5.1 `ToolApprovalPolicy::action_for()` — Extended Signature

**File:** `libs/agent-core/src/types.rs`

```rust
// Before
pub fn action_for(&self, tool_name: &str) -> ToolApprovalAction

// After
pub fn action_for(&self, tool_name: &str, tool_arguments: &serde_json::Value) -> ToolApprovalAction
```

When `tool_name` is `"run_command"` / `"run_command_task"` and `tool_arguments` contains a `"command"` field: the command string is parsed and resolved via `shell_tool_approvals::resolve_hierarchical_policy(...)`.

- `run_command_task` first checks `run_command_task...` scopes, then falls back to shared `run_command...` scopes if no task-specific rule matches.
- Parse failures fail closed to at least `Ask` while preserving stricter explicit `Deny` rules.
- All other tool names are unaffected.

### 5.2 `ApprovalStateMachine::new()` — Pass Arguments

**File:** `libs/agent-core/src/approval.rs`

```rust
// Before
policy.action_for(&tool_call.name)

// After
policy.action_for(&tool_call.name, &tool_call.arguments)
```

### 5.3 `AutoApproveManager` — Scope-Aware Approval

**File:** `tui/src/services/auto_approve.rs`

An early-exit scope resolution is inserted at the top of `get_policy_for_tool()`:

```rust
if tool_name == "run_command" || tool_name == "run_command_task" {
    if let Some(action) = resolve_shell_scope(tool_call, &self.config.tools, &self.config.default_policy) {
        return action;
    }
}
```

`resolve_shell_scope` now delegates to `shell_tool_approvals::resolve_hierarchical_policy(...)` using the tool name as the primary scope and `run_command` as a shared fallback scope for `run_command_task`.

If the command string cannot be extracted, it returns `None`. If parsing fails (for example due to excessive nested `-c` depth), it fails closed to at least `Prompt` while preserving stricter explicit `Never` rules.

**Note:** The hardcoded `run_command → Prompt` entries (in `AutoApproveConfig::default()` and the `ensure_command_patterns()` / approval-update paths) are **not removed** in this commit. `CommandPatterns` remains in the struct as dead code. Cleanup is deferred to a follow-up.

### 5.4 `handle_show_confirmation_dialog()` — No Changes

**File:** `tui/src/services/handlers/dialog.rs`

`ToolCall` already carries `function.arguments` as a JSON string; `resolve_shell_scope()` parses it internally. No call-site changes needed.

---

## 6. Configuration

Scope rules use **existing** configuration surfaces only. No new fields are introduced anywhere. Scope keys like `"run_command::ls"` are simply entries in the same `tools` / `rules` / `auto_approve` collections that already hold `"run_command"`.

### 6.1 Profile-Level (TOML)

**File:** `~/.stakpak/config.toml`

The existing `auto_approve` list in `ProfileConfig` (`cli/src/config/profile.rs`) accepts tool names as strings. Scope keys work the same way:

```toml
[profiles.default]
auto_approve = [
  "view",
  "search_docs",
  "run_command::ls",
  "run_command::cat",
  "run_command::echo",
  "run_command::git::status",
]
```

No struct changes to `ProfileConfig`. The existing `auto_approve: Option<Vec<String>>` field already carries these strings into `AutoApproveManager`, where they become `Auto` entries in the `tools` HashMap.

### 6.2 Session-Level (JSON)

**File:** `.stakpak/session/auto_approve.json`

Scope keys are entries in the existing `tools` object:

```json
{
  "enabled": true,
  "default_policy": "Prompt",
  "tools": {
    "view": "Auto",
    "run_command": "Prompt",
    "run_command::ls": "Auto",
    "run_command::cat": "Auto",
    "run_command::rm::-rf": "Never",
    "run_command::curl::--upload-file": "Never",
    "run_command::kubectl": "Auto"
  }
}
```

### 6.3 `ToolApprovalPolicy` Rules (Agent-Core)

**File:** `libs/agent-core/src/types.rs`

The existing `Custom { rules: HashMap<String, ToolApprovalAction> }` variant holds scope keys alongside regular tool names. No changes to the enum definition:

```rust
let mut rules = HashMap::new();
rules.insert("run_command".into(),          ToolApprovalAction::Ask);
rules.insert("run_command::ls".into(),      ToolApprovalAction::Approve);
rules.insert("run_command::rm::-rf".into(), ToolApprovalAction::Deny);
```
