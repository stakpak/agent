You have access to `ak`, a persistent knowledge store.
It stores markdown files in a directory that survives across sessions.

Key commands:
- ak search [path]            — recursively preview files with peek bodies
- ak search [path] --tree     — inspect structure only
- ak search [path] --grep ... — find files by matching content
- ak search [path] --glob ... — find files by matching paths
- ak read <path>...           — read full content
- ak write <path>             — create new knowledge file (stdin or -f <file>)
- ak remove <path>            — remove a knowledge file or directory

Files are immutable by default — `ak write` errors if the file
already exists. Use this for extracted facts and knowledge.
Use `--force` for mutable documents like summaries and indexes.

Organize however you want — directories, naming conventions,
frontmatter, cross-references. There are no rules.

Discovery flow: start with `ak search [path]`, narrow with
`--tree`, `--grep`, or `--glob` when needed, then use
`ak read` only for the files that matter.

If you synthesize an answer from multiple files, consider
writing the synthesis back as new knowledge.

Source-citation convention
--------------------------
When an entry is derived from a specific source (a session, a file
read, a command output, or another identified resource), cite the
source in YAML frontmatter under `sources:`. Each row carries three
required fields — `session` (UUID), `checkpoint` (UUID), and
`captured_at` (date in `YYYY-MM-DD` form) — plus an optional
`message_range` field reserved for entries pinned to specific turns
of a long session.

```yaml
---
description: Short sentence describing the entry.
sources:
  - session: 550e8400-e29b-41d4-a716-446655440000
    checkpoint: 6ba7b810-9dad-11d1-80b4-00c04fd430c8
    captured_at: 2026-04-24
    # message_range: "14-27"   # optional; only when pinned to turns
---
```

If a later source supports an entry that already exists, append a new
row to that file's existing `sources:` list and use `ak write --force`
to save the update. Do not write a second file for content that
belongs in an existing entry.

Citations are both the audit trail for every evidence-derived entry
and the idempotency anchor future retrospection scans to decide what
has already been processed. They are not optional on evidence-derived
writes.