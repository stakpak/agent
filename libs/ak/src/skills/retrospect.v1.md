You are running retrospection: turn recent `stakpak sessions` into
durable entries in the current `ak` store. The goal is long-term,
auditable agent knowledge — not a log, not a digest.

`ak` owns every convention you follow here: layout, what counts as
long-term, how evidence is cited, when cleanup runs. This prompt is a
thin orchestrator over those conventions. If anything below conflicts
with what `stakpak ak skill usage` says, `ak skill usage` wins.

## Step 1 — internalize ak's conventions

Run `stakpak ak skill usage` and read it end-to-end, including the
source-citation rules (frontmatter `sources:` list, required `session:`
/ `checkpoint:` / `captured_at:` fields, the append-don't-duplicate
rule, `write --force` for updates).

## Step 2 — orient in the store

Run `stakpak ak tree` to see the full structure. Then use
`stakpak ak peek` on likely containers — any `_schema.md`, project
roots, top-level indexes — to understand what is already known, how it
is organized, and which sessions are already cited. Collect the set of
every `session:` UUID that appears in any existing entry's frontmatter
`sources:` list; that set is your "already processed" reference for
this run.

Use `stakpak ak cat` when `peek` is not enough to judge whether a
specific existing entry already covers a topic you are about to touch.

## Step 3 — list candidate sessions

Run `stakpak sessions list --json`, paginating until you have the
recent window you intend to cover in this run. For each candidate,
keep `id`, `title`, `message_count`, `status`, `cwd`, and
`updated_at`.

Process candidates newest-first. Budget exhaustion (schedule timeout,
max-step caps, context pressure) is expected on large stores; newest-
first biases the run toward the most recent signal, and the citation
convention guarantees the next retrospection picks up the remainder
without rework.

## Step 4 — triage

Drop any session whose active checkpoint is already in the cited-session
set from Step 2. For the remainder, open a session only when both of
these hold:

- its `title` plausibly describes substantive technical work, and
- its content is likely to add signal beyond what the store already
  covers, based on what you saw in Step 2.

Soft floor: sessions with fewer than about 4 messages rarely carry
durable signal; skip them unless the title is exceptionally specific.
A `status` of `error` or `failed` is a positive signal — failure modes
are often exactly what you want to capture — not a reason to filter.

## Step 5 — open and extract

For each session that survives triage, run
`stakpak sessions show <id> --json` to fetch the full message history.
Read through it and decide what, if anything, is durable. Do not work
from a fixed taxonomy for "worth extracting"; use the shape,
granularity, and subject matter of existing `ak` entries as your live
reference for what qualifies. When in doubt, one well-cited atomic
fact beats a vague summary.

Treat each session's `cwd` as organizational context, not a filter.
Consult the current `ak` schema to decide whether an insight belongs
under a project-scoped path or a universal one. If `ak` is organized
per-project, write per-project; if flat, write flat. Do not restrict
yourself to the current `cwd`.

Never extract secret-shaped content — tokens, passwords, API keys,
credentials, signed URLs, private keys — even when it appears in tool
output inside a session. If a durable fact is only meaningful with a
secret attached, redact the secret or skip the fact.

## Step 6 — write with citations

Every entry you write MUST cite its source(s) per the `ak skill usage`
convention:

```yaml
---
description: Short sentence describing the entry.
sources:
  - session: 550e8400-e29b-41d4-a716-446655440000
    checkpoint: 6ba7b810-9dad-11d1-80b4-00c04fd430c8
    captured_at: 2026-04-24
---
```

If the subject already has a home in `ak`, append a new row to that
file's existing `sources:` list and use `stakpak ak write --force` to
save. Do not create a new file for content that belongs in an existing
entry. Add the optional `message_range` only when the entry is pinned
to specific turns of a long session.

When newer evidence conflicts with an older cited claim, prefer the
newer evidence: supersede the older content with
`stakpak ak write --force`, date the update via the new source's
`captured_at`, and keep the prior citation in the `sources:` list so
the trail of how the fact evolved remains intact.

## Step 7 — discipline the store

After you finish writing, run `stakpak ak skill maintain` and apply
its guidance to the entries you just touched and their neighbors. A
batch of fresh writes is exactly when consolidation, dedupe, and
staleness-pruning are cheapest.

## Step 8 — report

Print a terse plain-text summary at the end of the run:

- how many sessions were listed, triaged, opened, and skipped, plus
  the dominant skip reason (already cited, too short, off-topic);
- how many `ak` entries were created vs. updated, grouped by top-level
  path;
- any conflicts you surfaced for the user to adjudicate.

Density beats prose — this lands in autopilot run logs on scheduled
runs.

---

Schedule this skill via the canonical one-liner:

    stakpak autopilot schedule add --name retrospect --cron "0 3 * * *" --prompt "$(stakpak ak skill retrospect)"
