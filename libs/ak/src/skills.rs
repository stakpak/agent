pub const SKILL_USAGE: &str = r#"You have access to `ak`, a persistent knowledge store.
It stores markdown files in a directory that survives across sessions.

Key commands:
- ak tree / ak ls        — see what exists (structure and listings)
- ak peek <path>         — read summary (frontmatter + first paragraph)
- ak cat <path>          — read full content
- ak write <path>        — create new knowledge file (stdin or -f <file>)
- ak write --force <path> — overwrite an existing file
- ak rm <path>           — remove a knowledge file

Files are immutable by default — `ak write` errors if the file
already exists. Use this for extracted facts and knowledge.
Use `--force` for mutable documents like summaries and indexes.

Organize however you want — directories, naming conventions,
frontmatter, cross-references. There are no rules.

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
writes."#;

pub const SKILL_RETROSPECT: &str = include_str!("skills/retrospect.v1.md");

pub const SKILL_MAINTAIN: &str = r#"Review your knowledge store for quality and accuracy.

1. Run `ak tree` and `ak ls` to see what exists.
2. Look for:
   - Duplicate or near-duplicate entries → write a merged version,
     remove the originals
   - Contradictory facts → resolve or flag to the user
   - Stale information (old dates, outdated facts) → remove and
     write corrected versions
   - Scattered facts that should be consolidated into a single file
   - Overly broad files that should be split into atomic facts
3. Use `ak write`, `ak write --force`, and `ak rm` to fix what
   you find.
4. Summarize what you changed and why."#;

#[cfg(test)]
mod tests {
    use super::{SKILL_RETROSPECT, SKILL_USAGE};

    const AUTOPILOT_ONE_LINER: &str = r#"stakpak autopilot schedule add --name retrospect --cron "0 3 * * *" --prompt "$(stakpak ak skill retrospect)""#;

    #[test]
    fn retrospect_skill_matches_bundled_markdown() {
        // Golden-file: the constant must be exactly the bundled file byte-for-byte.
        // If someone wraps `include_str!` with `.trim()` / `.replace(...)` / etc.,
        // this test will fail.
        let bundled = include_str!("skills/retrospect.v1.md");
        assert_eq!(
            SKILL_RETROSPECT, bundled,
            "SKILL_RETROSPECT must match bundled retrospect.v1.md byte-for-byte"
        );
        assert!(
            SKILL_RETROSPECT.starts_with("You are running retrospection:"),
            "SKILL_RETROSPECT appears to have been trimmed or mutated at its head"
        );
        assert!(
            SKILL_RETROSPECT.trim_end().ends_with(
                r#"stakpak autopilot schedule add --name retrospect --cron "0 3 * * *" --prompt "$(stakpak ak skill retrospect)""#
            ),
            "SKILL_RETROSPECT appears to have been trimmed or mutated at its tail"
        );
    }

    #[test]
    fn retrospect_skill_contains_required_substrings() {
        for needle in [
            "stakpak ak skill usage",
            "stakpak ak skill maintain",
            "stakpak sessions list --json",
            "stakpak sessions show",
            "sources:",
            "session:",
            "checkpoint:",
            "captured_at:",
            "write --force",
            "newest-first",
            AUTOPILOT_ONE_LINER,
        ] {
            assert!(
                SKILL_RETROSPECT.contains(needle),
                "SKILL_RETROSPECT is missing required substring: {needle:?}"
            );
        }
    }

    #[test]
    fn retrospect_skill_has_no_forbidden_substrings() {
        // Hard-forbidden markers of a pinned retrospect-owned layout.
        for needle in ["retrospect/<", "patterns/<", "_index.md"] {
            assert!(
                !SKILL_RETROSPECT.contains(needle),
                "SKILL_RETROSPECT contains forbidden substring: {needle:?}"
            );
        }
        // Antipattern check: an enumerated numbered list of "things worth
        // writing" (decisions / failure modes / verified facts / ...) would
        // pre-enumerate content categories instead of deferring to ak. The
        // prompt must explicitly tell the agent NOT to follow a fixed
        // taxonomy and must point the agent at existing ak entries as the
        // live reference for what qualifies.
        assert!(
            SKILL_RETROSPECT.contains("fixed taxonomy"),
            "SKILL_RETROSPECT must reject a fixed taxonomy for 'worth extracting'"
        );
        assert!(
            SKILL_RETROSPECT.contains("existing `ak` entries"),
            "SKILL_RETROSPECT must point the agent at existing ak entries as its reference"
        );
    }

    #[test]
    fn usage_skill_contains_citation_convention() {
        for needle in [
            "sources:",
            "session",
            "checkpoint",
            "captured_at",
            "message_range",
            "audit trail",
            "idempotency",
        ] {
            assert!(
                SKILL_USAGE.contains(needle),
                "SKILL_USAGE is missing required citation-convention substring: {needle:?}"
            );
        }
    }
}
