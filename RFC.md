# RFC 006: Session Side Panel & Recovery

## Summary

This RFC proposes a **Session Side Panel** and **Session Recovery** system for the Stakpak CLI (TUI). The side panel provides a focused view of essential session information, while recovery ensures resilience against crashes.

## Motivation

### Problem Statement

1. **Context Blindness**: Users lose track of token usage, remaining credits, and overall session state.
2. **Task Tracking**: No persistent view of what needs to be done (todos) or scratch notes.
3. **Change Awareness**: Hard to see which files have been modified and navigate their edit history.
4. **Session Fragility**: Crashes or closures lose session state.

### Design Principle: Information Minimalism

> **Every piece of information in the side panel must justify its presence.**
> 
> We are NOT building a timeline of all tool calls. We are building a focused dashboard of *actionable* information.

---

## Side Panel Design

The side panel contains **four collapsible sections**, ordered by importance:

### 1. Context Information (Always Visible)
**Justification**: Users need to know resource consumption to avoid surprises.

```
┌── Context ────────────────────────────┐
│  Tokens:    12,450 / 128K  (9.7%)     │
│  Credits:   $2.34 remaining           │
│  Session:   15m 23s                   │
│  Model:     claude-sonnet-4-5-20250929            │
└───────────────────────────────────────┘
```

### 2. Todos (Collapsible, Expanded by Default)
**Justification**: Tracks what the agent is working on and what's pending.

```
┌── Todos ──────────────────────────────┐
│  [x] Create database schema           │
│  [/] Implement user authentication    │
│  [ ] Add API endpoints                │
│  [ ] Write tests                      │
└───────────────────────────────────────┘
```
- `[x]` = Done
- `[/]` = In Progress
- `[ ]` = Pending

**Source**: Parsed from `task.md` if present, or agent-generated task breakdowns.

### 3. Changeset (Collapsible, Collapsed by Default)
**Justification**: Know what files changed without scrolling through chat.

```
┌── Changeset (4 files) ────────────────┐
│  ▸ src/auth.rs        (+45, -12)      │
│  ▸ src/main.rs        (+3, -1)        │
│  ▸ Cargo.toml         (+2, -0)        │
│  ▸ README.md          (+20, -5)       │
└───────────────────────────────────────┘
```

**Expanded File View** (on Enter):
```
┌── src/auth.rs (3 edits) ──────────────┐
│  10:42  Added login function          │
│  10:45  Fixed password validation     │
│  10:48  Added session handling        │
│  ───────────────────────────────────  │
│  [View Diff] [Revert to Edit #2]      │
└───────────────────────────────────────┘
```

### 4. Scratchpad (Collapsible, Collapsed by Default)
**Justification**: Persistent notes that survive the scrolling chat.

```
┌── Scratchpad ─────────────────────────┐
│  API endpoint: localhost:8080         │
│  Test user: admin@example.com         │
│  Remember: run migrations first       │
└───────────────────────────────────────┘
```

**Source**: User can add notes via `/note` command or agent can append important findings.

---

## What We're NOT Including

| Excluded Item | Reason |
|---------------|--------|
| Full tool call timeline | Too noisy. Tool calls are visible in chat. |
| Every checkpoint | Checkpoints are internal. Users care about file states. |
| Agent thinking/reasoning | Available in chat, adds clutter. |
| Detailed diffs in panel | Too dense. Available on-demand via Changeset. |

---

## Layout

```
┌────────────────── Main Chat (70%) ──────────────────┐┌───── Side Panel (30%) ─────┐
│                                                     ││ ┌── Context ─────────────┐ │
│ > User: Set up authentication                       ││ │ Tokens: 12K/128K       │ │
│                                                     ││ │ Credits: $2.34         │ │
│ 🤖 I'll create the auth module...                   ││ └────────────────────────┘ │
│                                                     ││ ┌── Todos ───────────────┐ │
│ [Tool: write_file src/auth.rs]                      ││ │ [x] Create schema      │ │
│ ✓ Created src/auth.rs                               ││ │ [/] Auth module        │ │
│                                                     ││ │ [ ] API endpoints      │ │
│                                                     ││ └────────────────────────┘ │
│                                                     ││ ▸ Changeset (2 files)      │
│                                                     ││ ▸ Scratchpad               │
└─────────────────────────────────────────────────────┘└────────────────────────────┘
  Status: main │ Mode: EXECUTION │ Ctrl+B: Toggle Panel
```

---

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+B` | Toggle side panel visibility |
| `Ctrl+G` | Focus Context section (existing) |
| `Tab` | Cycle between sections |b
| `Enter` | Expand/collapse section or file |
| `j/k` | Navigate within focused section |
| `r` | Revert file to previous edit (in Changeset) |

---

## Session Recovery

### Continuous Persistence
- Session state saved to `~/.stakpak/sessions/<uuid>.json` on every event
- Includes: chat history, todos, changeset, scratchpad

<!-- ### Crash Resume
On startup, detect unclean shutdowns:
```
Detected interrupted session from 2m ago.
[R]esume  [N]ew session  [D]iscard
``` -->

### File Rollback
Via Changeset panel:
1. Expand file → Select edit point → Press `r`
2. Confirmation: "Revert src/auth.rs to edit #2?"
3. System restores file and updates changeset

---

## Implementation Phases

### Phase 1: Side Panel UI
- [ ] Implement horizontal split in `view.rs`
- [ ] Create collapsible section widget
- [ ] Render Context, Todos, Changeset, Scratchpad sections

### Phase 2: Data Integration
- [ ] Hook context/credit tracking into Context section
- [ ] Parse `task.md` for Todos (or track agent-generated tasks)
- [ ] Track file modifications for Changeset
- [ ] Implement `/note` command for Scratchpad

### Phase 3: Session Persistence
- [ ] Create `libs/session` crate
- [ ] Implement file-based session storage
- [ ] Add startup recovery flow

### Phase 4: Rollback
- [ ] Store file snapshots on modification
- [ ] Implement revert logic from Changeset

---

## Open Questions

1. **Scratchpad persistence**: Should scratchpad notes persist across sessions or be session-scoped?
2. **Todo source**: Generate from agent task planning, or require explicit `task.md`?
3. **Changeset depth**: How many edits per file to track? (Proposal: 10)