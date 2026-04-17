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
writing the synthesis back as new knowledge."#;

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
