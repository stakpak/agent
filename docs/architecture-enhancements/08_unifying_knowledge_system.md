# RFC: Unified Skills Architecture

## Summary

Replace Stakpak's fragmented knowledge system (Rulebooks, Paks, and missing local skills) with a unified **Skills** architecture. A single `Skill` type represents all knowledge sources — local filesystem skills, remote vetted rulebooks, and community paks — with progressive disclosure, configurable discovery directories, and a consistent user experience across CLI, TUI, and MCP tools.

## Motivation

### Current Problems

Stakpak has three different concepts that serve similar purposes:

| Concept | Source | Trust | UX Coverage | Name in Code |
|---------|--------|-------|-------------|--------------|
| Rulebooks | Remote API | Vetted by Stakpak/org | CLI, TUI, config, prompts | `ListRuleBook`, `RuleBook` |
| Paks | Remote registry | Unvetted, requires approval | MCP tools only | `paks__*` tools |
| Local skills | Filesystem | User-created | **None (missing)** | N/A |

**Problems identified:**

1. **Naming inconsistency** — Three names for the same concept. Confusing for users and scattered across the codebase.
2. **Fragmented UX** — Rulebooks have full CLI/TUI/config support; Paks are MCP-only; local skills don't exist.
3. **No local skills** — Users cannot create, organize, or version-control their own knowledge.


### Goals

- Unified `Skill` type across all sources
- Progressive disclosure: metadata at startup, content on-demand via `load_skill` tool
- Constant skill directories (project-level, user-level)
- Backward compatibility with existing rulebook configs and commands
- Single consistent UX across CLI, TUI, and agent runtime

---

## Design

### Core Type: `Skill`

**Location:** `libs/api/src/skills.rs`

```rust
pub struct Skill {
    pub id: String,          // e.g., "terraform-aws" or "stakpak/security"
    pub uri: String,         // Unique identifier / API URI / FilePath
    pub description: String, // When to use this skill
    pub source: SkillSource, // Local or Remote (Pak/Rulebook)
    pub content: Option<String>, // None = metadata only (progressive disclosure)
    pub tags: Vec<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

pub enum SkillSource {
    Local,
    Remote { provider: RemoteProvider },
}

pub enum RemoteProvider {
    Rulebook { visibility: RuleBookVisibility },
    Pak,
}
```

**Helper methods:** `is_local()`, `is_rulebook()`, `is_pak()`

**Conversion traits:**
- `From<ListRuleBook> for Skill` — metadata only (no content)
- `From<RuleBook> for Skill` — with content
- `TryFrom<Skill> for RuleBook` — reverse conversion for API calls
- `TryFrom<Skill> for ListRuleBook` — reverse conversion for API calls

### SKILL.md Format

Each local skill lives in a directory containing a `SKILL.md` file:

```
.stakpak/skills/
  terraform-aws/
    SKILL.md
    scripts/        # Optional helper scripts
    references/     # Optional reference files
    assets/         # Optional assets
```

SKILL.md uses YAML frontmatter:

```markdown
---
name: terraform-aws
description: Best practices for Terraform on AWS
tags: [terraform, aws, iac]
---

# Terraform AWS Instructions

Step-by-step guidance here...
```

**Required fields:** `name`, `description`
**Optional fields:** `tags` (defaults to `[]`)

### Progressive Disclosure

The system uses a two-phase approach to minimize context usage:

1. **Discovery phase** (startup): Only frontmatter is parsed. Skill metadata (name, description, tags) is injected into the first user message as an `<skills>` XML block.

2. **Activation phase** (on-demand): When the LLM needs a skill, it calls the `load_skill` MCP tool (auto-approved). The full SKILL.md body (frontmatter stripped) is returned along with the skill directory path.

```
Startup injection (metadata only):
<skills>
# Available Skills:
- [📁 local] terraform-aws: Best practices for Terraform on AWS [terraform, aws, iac]
- [☁️ rulebook] security-baseline: Organization security requirements [security]
</skills>

On-demand loading (full content):
Agent calls: load_skill(name: "terraform-aws")
Returns: Skill directory: .stakpak/skills/terraform-aws

# Terraform AWS Instructions
...full content...
```

### Skill Discovery

**Function:** `discover_skills(directories: &[PathBuf]) -> Vec<Skill>`

- Scans each directory for subdirectories containing `SKILL.md`
- Parses only frontmatter (progressive disclosure)
- First skill with a given name wins (project-level overrides user-level)
- Directories that don't exist are silently skipped

**Default directories (in priority order):**
1. `.stakpak/skills/` — project-level (relative to working directory)
2. `~/.config/stakpak/skills/` — user-level (global)



**Resolution in `AppConfig::build()`:**


### MCP Tool: `load_skill`

**Location:** `libs/mcp/server/src/remote_tools.rs`

```rust
#[tool(description = "Load a skill's full instructions by uri...")]
async fn load_skill(&self, uri: String) -> Result<CallToolResult, McpError> {
}
```

- Auto-approved (no user confirmation required)
- Case-insensitive name matching
- Returns skill directory path + stripped markdown body

### Prompt Injection: `add_skills`

**Location:** `cli/src/commands/agent/run/helpers.rs`

Formats all active skills into an `<skills>` block with source indicators:

| Source | Example |
|--------|------|---------|
| Local  | `[local] my-skill: Description` |
| Rulebook | `[rulebook] security: Org security rules` |

Injected into the first user message (or when skills are updated via the TUI switcher). The TUI Switcher won't change it is only for rulebooks for now 

### Unified Skills in Agent Runtime

**Interactive mode** (`mode_interactive.rs`):
1. Convert rulebooks API response → `Vec<Skill>` via `Skill::from()`
2. Discover local skills via `discover_skills(skill_directories)`
3. Merge: `skills.extend(local_skills)`
4. Inject metadata via `add_skills()` on first message
5. TUI rulebook switcher filters: keeps all local + selected remote

**Async mode** (`mode_async.rs`):
1. Receives `skills: Option<Vec<Skill>>` in `RunAsyncConfig`
2. Injects via `add_skills()` when chat is empty

### Backward Compatibility

| Old Concept | New Concept | Migration |
|-------------|-------------|-----------|
| `ListRuleBook` type | `Skill` with `source: Remote { Rulebook }` | `From` trait conversion |
| `add_rulebooks()` | `add_skills()` | Function replaced |
| `stakpak rulebooks` CLI | `stakpak rulebooks` CLI (kept) | Future: alias for `stakpak skills --source remote` |
| TUI `RulebooksLoaded` event | `RulebooksLoaded` event (kept) | TUI still uses for switcher |
| `RequestRulebookUpdate` | `RequestRulebookUpdate` (kept) | Filters skills by source |

---

## Implementation Status

### Phase 1: Local Skills + Unified Type (Complete)

**Core types** (`libs/api/src/skills.rs`):
- [x] `Skill` struct with `SkillSource` discriminator
- [x] `RemoteProvider` enum (Rulebook, Pak)
- [x] Conversion traits: `From<ListRuleBook>`, `From<RuleBook>`, `TryFrom<Skill>`
- [x] Helper methods: `is_local()`, `is_rulebook()`, `is_pak()`

**Local skills** (`libs/api/src/local/skills/`):
- [x] `parser.rs`: YAML frontmatter parser with `SkillFrontmatter` struct
- [x] `mod.rs`: `discover_skills()`, `load_skill()`, `load_skill_from_path()`

**MCP tool** (`libs/mcp/server/src/`):
- [x] `load_skill` tool in `remote_tools.rs`
- [x] `read_rulebook` remove and merge its logic in `load_skill`
- [x] Auto-approved in `lib.rs`
- [x] `ToolContainer.skill_directories` field with defaults

**CLI integration** (`cli/src/commands/agent/run/`):
- [x] `helpers.rs`: `add_skills()` replaces `add_rulebooks()`
- [x] `mode_interactive.rs`: Unified skills flow (convert rulebooks + discover local + merge)
- [x] `mode_async.rs`: `RunAsyncConfig.skills` replaces `rulebooks`
- [x] `main.rs`: Converts `rulebooks_result` → `initial_skills` via `Skill::from()`

### Phase 2: System Prompts (Complete)

- [x] Update system prompts to reference unified skills taxonomy

### Phase 3: CLI Commands (Future)

- [ ] `stakpak skills list [--source local|remote|all]`
- [ ] `stakpak skills get <id>`
- [ ] `stakpak skills apply <path>`
- [ ] `stakpak skills delete <id>`
- [ ] Keep `stakpak rulebooks` as alias with deprecation notice

### Phase 4: Deprecation (Future)

- [ ] Add deprecation warnings to `stakpak rulebooks` commands
- [ ] Migrate `[rulebooks]` config → `[skills.remote]` config
- [ ] Remove old naming in a major version



## Old System Prompt (Reference)

```
# Knowledge Sources: Rulebooks and Paks

You have access to two complementary knowledge systems:

## 1. Rulebooks (User-Specific)
Rulebooks are provided at session start and contain guidelines, procedures, and
instructions specific to the user's environment. Always check available rulebooks
first for task-relevant guidance.

## 2. Paks (Community Knowledge)
Paks are community-contributed skill packages from the Stakpak registry. Use paks when:
- Available rulebooks don't cover the topic adequately
- You need additional best practices or procedures
- The task involves common DevOps patterns that may have community solutions
```


