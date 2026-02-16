# Write RFC

Write an RFC for a new feature or architectural change.

## Before Writing

1. **Study existing RFCs** for structure and style:
   - `docs/rfcs/rfc_stakpak_init.md` — implementation RFC with diagrams
   - `docs/rfcs/rfc_extend_commands.md` — feature RFC with user flows

2. **Review architecture context**:
   - `docs/architecture-enhancements/` — existing proposals
   - `AGENTS.md` — codebase structure and patterns

## RFC Structure

```markdown
# RFC: [Feature Name]

> **Status**: Draft | Under Review | Accepted | Implemented
> **Author**: [Name]
> **Date**: [Date]

## Summary
[2-3 sentences: what and why]

## Motivation
**Problem**: [What's broken or missing]
**Goals**: [What success looks like]
**Non-goals**: [What this explicitly won't do]

## User Experience
**CLI**: [Commands/flags]
**TUI**: [UI changes]
**Example**: [Concrete usage example]

## Design

### Architecture
[ASCII diagram of component flow]

### Key Components
| Component | File | Purpose |
|-----------|------|---------|
| [Name] | `path/file.rs` | [What it does] |

### Data Flow
[How data moves through the system]

## Implementation
[Key code patterns, snippets for complex parts]

## Design Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| [Topic] | [What] | [Why] | [What else considered] |

## Future Work
[What's deferred, known limitations]

## References
[Related issues, docs, external resources]
```

## Tips
- ASCII diagrams for flows (see existing RFCs)
- Reference specific files in codebase
- Consider both CLI and TUI entry points
- Document the "why" behind decisions

---

**Start**: What feature? I'll research the codebase, identify affected components, and draft the RFC.
