# Slash Command Extensibility Across Coding Agents
## Competitive Analysis for Stakpak `/stakpak-init` Design

---

## Executive Summary

Every major coding agent now supports user-extensible slash commands via **markdown files on disk**. Claude Code pioneered the pattern; OpenCode, Cursor, and GitHub Copilot CLI all converged on nearly identical designs. OpenHands is actively building toward the same model. Aider remains the outlier — it has no native custom command system (only hardcoded built-in commands).

The key takeaway for Stakpak: **Stakpak's current slash command system is hardcoded in Rust (`commands.rs` match block)**, which is the least extensible approach in the market. Every competitor has moved to file-based discovery. A `/stakpak-init` command is useful, but the bigger opportunity is adopting a file-based extensibility model.

---

## Detailed Comparison

### 1. Claude Code (Anthropic) — The Market Leader

**Mechanism:** Markdown files in `.claude/commands/` (project) or `~/.claude/commands/` (personal)

**How it works:**
- File name becomes command name: `review.md` → `/review`
- Subdirectories create namespaces: `frontend/component.md` → `/project:frontend:component`
- YAML frontmatter controls behavior:
  ```yaml
  ---
  description: "Run security vulnerability scan"
  allowed-tools: Read, Grep, Glob
  model: claude-opus-4-6
  argument-hint: [issue-number] [priority]
  ---
  ```
- `$ARGUMENTS` placeholder for all args, `$1`, `$2` for positional args
- `allowed-tools` in frontmatter restricts which tools the command can use
- Commands auto-appear in autocomplete when user types `/`

**Recent evolution (Skills system):**
- Commands have been **merged into Skills** — `.claude/skills/review/SKILL.md` and `.claude/commands/review.md` both create `/review`
- Skills add: directory for supporting files, frontmatter to control model vs. user invocation, auto-loading when relevant
- Skills follow the **Agent Skills open standard** (cross-tool compatibility)
- `disable-model-invocation: true` prevents the AI from invoking the skill autonomously
- Permission control: `Skill(commit)` for exact match, `Skill(name *)` for prefix match

**Ecosystem:**
- 148+ community commands (Claude-Command-Suite)
- 57 production-ready commands (wshobson/commands)
- Plugin marketplace for distribution
- awesome-claude-code curated list

**Key differentiator:** Skills can be **automatically invoked by the model** when contextually relevant, not just user-triggered. This is unique — no other agent does this.

---

### 2. OpenCode (SST) — Go-based, Most Feature-Rich Commands

**Mechanism:** Markdown files in three locations:
- `.opencode/commands/` (project)
- `~/.config/opencode/commands/` (personal)
- JSON config in `opencode.json` under `"command"` key

**How it works:**
- File name becomes command ID: `prime-context.md` → `/user:prime-context`
- Scoping: `project:` prefix for project commands, `user:` for personal
- **Named arguments** with `$NAME` syntax (uppercase, underscores allowed):
  ```markdown
  # Fetch Context for Issue $ISSUE_NUMBER
  RUN gh issue view $ISSUE_NUMBER --json title,body,comments
  RUN git grep --author="$AUTHOR_NAME" -n .
  ```
- When user runs command with named args, OpenCode prompts for each placeholder
- `!command` syntax injects **bash output into prompts**:
  ```markdown
  !git diff --name-only
  Based on these results, suggest improvements.
  ```
- `@filename` syntax includes file contents
- Custom commands can **override built-in commands**
- YAML frontmatter with `description` field

**Additional extensibility:**
- **Custom Tools** (TypeScript/JavaScript): `tool()` helper with Zod schemas for argument types
- **Plugins** (`.opencode/plugins/`): JS/TS files that intercept events and tool executions
- **Rules** (`AGENTS.md`): Persistent instructions in LLM context
- **Custom Agents**: Full agent definitions with custom system prompts, tool access, models

**Key differentiator:** The `!command` bash injection and `@file` inclusion in markdown commands are unique to OpenCode. Also the only agent that supports named argument prompting (not just positional).

---

### 3. Cursor IDE — Editor-Integrated Commands

**Mechanism:** Markdown files in `.cursor/commands/`

**How it works:**
- File name becomes command name
- Triggered by typing `/` in Agent chat input
- Commands appear in dropdown for selection
- Stored as markdown with standard structure:
  ```markdown
  # Generate API Documentation
  Create comprehensive API documentation for the current code.
  Include:
  - Endpoint descriptions and HTTP methods
  - Request/response schemas with examples
  ```
- Version-controlled, shareable via git

**Recent evolution (Skills system, Cursor 2.4+):**
- **Agent Skills** via `SKILL.md` files — same Agent Skills open standard as Claude Code
- Skills can be invoked via slash command menu OR automatically by the agent
- Custom **subagents** with their own prompts, tool access, and models
- **Rules** (`.cursor/rules/`) for persistent instructions — can emulate slash commands via prompt engineering

**Unique approach:** Some developers create "fake" slash commands by defining rules like:
```
## Slash Commands
/s <term> = Use the codebase_search tool to search for <term>
/l = Use the edit_file tool to update cursor rules with learnings
```
The AI interprets these as instructions, not native commands.

**Key differentiator:** Deep IDE integration — commands can trigger editor-specific actions (inline edit, file navigation, terminal commands) that terminal-based agents can't.

---

### 4. GitHub Copilot CLI — Plugin-Based Architecture

**Mechanism:** Plugin system with Skills bundled inside plugins

**How it works:**
- Plugins installed via `/plugin install` slash command
- Plugin directories contain skills in `skills/<name>/SKILL.md`
- Skills auto-register as slash commands
- Plugin tracking in `~/.copilot/plugin-index.json`
- `plugin.json` manifest defines metadata
- Plugin MCP servers auto-loaded on install

**Architecture:**
```
~/.copilot/plugins/analysis-plugin/
├── plugin.json
├── agents/
│   └── code-review.md
├── skills/
│   ├── code-review/
│   │   └── SKILL.md
│   └── security-audit/
│       └── SKILL.md
└── mcp-config.json
```

**Priority system for name collisions:**
1. Local agents (`~/.copilot/agents/`) — highest
2. Plugin agents — by install order
3. Built-in agents — lowest

**Key differentiator:** First-class **plugin distribution** with MCP server bundling. Plugins can provide tools, agents, AND commands as a single installable package.

---

### 5. OpenHands — Microagent-Based (Emerging)

**Current state:** Custom commands via **microagents** with `/` trigger prefix

**How it works:**
- Microagents are markdown files with keyword triggers
- If a trigger starts with `/`, it becomes a slash command
- Skills loaded from `.openhands/skills/` or compatible formats (`.cursorrules`, `agents.md`)
- Skills can be always-active (`trigger=None`) or keyword-activated
- Skills may include MCP tools

**In-progress (Issue #9927):**
- Allow creation of custom commands when user starts typing `/` in CLI and Web UI
- Autocomplete in both CLI and Web UI
- Refactor built-in CLI slash commands to be microagents
- Plugin system for distributing commands

**SDK architecture:**
- `AgentContext` centralizes all inputs including skills
- Sub-agent delegation for hierarchical coordination
- Event-sourced state management
- Multi-layer security with LLM-based risk assessment

**Key differentiator:** The microagent abstraction — commands aren't just prompts, they're mini-agents with their own behavior, tool access, and lifecycle.

---

### 6. Aider — No Custom Commands

**Current state:** Hardcoded built-in commands only (`/add`, `/drop`, `/model`, `/run`, `/undo`, etc.)

**Community requests:**
- Issue #894 (Jul 2024): Requested YAML-based custom commands with variable substitution
- Issue #4235 (Jun 2025): User asked for `/trans` custom command — no native support
- External tool `sirasagi62/slash`: Go utility for managing markdown prompts, works alongside Aider

**Extensibility model:** Aider relies on:
- Configuration via YAML/env files (model settings, conventions)
- `.aider.conf.yml` for persistent settings
- Editor conventions files for coding standards
- No plugin system, no custom commands, no skills

**Key insight:** Aider's simplicity is intentional — it focuses on being the best at one thing (AI pair programming) rather than being extensible. But the market is moving toward extensibility.

---

## Feature Matrix

| Feature | Claude Code | OpenCode | Cursor | Copilot CLI | OpenHands | Aider | **Stakpak** |
|---------|-------------|----------|--------|-------------|-----------|-------|-------------|
| Custom slash commands | ✅ Markdown | ✅ Markdown + JSON | ✅ Markdown | ✅ via Plugins | 🔄 Building | ❌ | ❌ Hardcoded |
| Command discovery location | `.claude/commands/` | `.opencode/commands/` | `.cursor/commands/` | Plugin skills | `.openhands/skills/` | N/A | `commands.rs` |
| Personal (global) commands | ✅ `~/.claude/commands/` | ✅ `~/.config/opencode/commands/` | ❌ | ✅ `~/.copilot/` | ✅ | N/A | ❌ |
| Argument support | ✅ `$1` `$ARGUMENTS` | ✅ `$NAME` named args | ❌ | ✅ `$1` `$ARGUMENTS` | 🔄 | N/A | ❌ |
| Tool restrictions per command | ✅ `allowed-tools` | ❌ | ❌ | ✅ via skills | ✅ | N/A | ❌ |
| Model override per command | ✅ `model:` frontmatter | ❌ | ❌ | ❌ | ✅ | N/A | ❌ |
| Bash injection in prompts | ❌ | ✅ `!command` | ❌ | ❌ | ❌ | N/A | ❌ |
| File inclusion in prompts | ❌ | ✅ `@filename` | ❌ | ❌ | ❌ | N/A | ❌ |
| Override built-in commands | ❌ | ✅ | ❌ | Priority-based | ❌ | N/A | ❌ |
| Auto-invocation by AI | ✅ Skills | ❌ | ✅ Skills | ✅ Skills | ✅ Microagents | N/A | ❌ |
| Plugin/distribution system | ✅ Marketplace | ✅ npm plugins | ❌ | ✅ First-class | 🔄 | N/A | ❌ |
| Namespace support | ✅ Subdirectories | ✅ `project:`/`user:` | ❌ | ✅ Plugin-scoped | ❌ | N/A | ❌ |

---

## Recommendations for Stakpak

### Immediate (for `/stakpak-init`)
The current approach of hardcoding in `commands.rs` is fine for built-in commands. The `/stakpak-init` RFC is sound — it follows **Pattern D (Populate Input with Text)** which is appropriate for an initialization command.

### Medium-term: File-Based Command Discovery
Stakpak should adopt the industry-standard pattern:
1. **Project commands**: `.stakpak/commands/*.md`
2. **Personal commands**: `~/.stakpak/commands/*.md`
3. **Discovery**: Scan directories at TUI startup, add to `HelperCommand` list
4. **Execution**: Parse markdown, substitute `$ARGUMENTS`, inject as `OutputEvent::UserMessage`
5. **Frontmatter**: Support `description`, `allowed-tools` at minimum

### Long-term: Skills/Paks Integration
Given Stakpak's existing "paks" concept (cloud-based tools), the extensibility model could be:
- **Skills** = local markdown commands (like Claude Code)
- **Paks** = cloud-hosted tool packages (existing)
- **Rulesets** = persistent instructions (existing via rulebooks)

This would position Stakpak competitively against all major agents while leveraging its unique cloud infrastructure advantage.

### Architecture Impact
Adding file-based commands requires minimal changes:
1. Add a `custom_commands: Vec<HelperCommand>` to `AppState`
2. Add a `scan_custom_commands()` function called during TUI init
3. In `execute_command()`, check custom commands before the match block
4. Custom commands resolve to `OutputEvent::UserMessage(rendered_prompt)`

This is additive — no existing code needs to change, and the hardcoded commands remain as built-ins with higher priority.