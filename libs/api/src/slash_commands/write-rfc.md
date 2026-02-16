# Write RFC

Write an RFC (Request for Comments) for a new feature or architectural change.

## Before Writing

1. **Study existing RFCs** in the repo for structure and style (e.g. `docs/rfcs/`, `RFC*.md`)
2. **Review architecture docs** — design docs, ADRs, or similar in the codebase

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
[Commands, APIs, UI changes — adapt to the project]
**Example**: [Concrete usage example]

## Design

### Architecture
[Diagram of component flow — ASCII or Mermaid]

### Key Components
| Component | Location | Purpose |
|-----------|----------|---------|
| [Name] | `path/to/file` | [What it does] |

### Data Flow
[How data moves through the system]

## Implementation
[Key patterns, code snippets for complex parts]

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

- Use diagrams for flows
- Reference specific files in the codebase
- Document the "why" behind decisions
- Adapt sections to the project (CLI, API, UI, etc.)

---

**Start**: What feature? I'll research the codebase, identify affected components, and draft the RFC.
