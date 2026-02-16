# Write PR Description

Generate a PR description for changes in this branch vs `origin/main`.

## Gather Context

```bash
git fetch origin
git diff origin/main...HEAD --stat
git log origin/main..HEAD --oneline
git diff origin/main...HEAD
```

Also check:
- `docs/rfcs/` — related RFCs being implemented
- `docs/architecture-enhancements/` — related proposals
- `CONTRIBUTING.md` — commit message format

## Output Format

```markdown
## Summary
[1-2 sentences: what this PR does and why]

## Related
- RFC: `docs/rfcs/rfc_xxx.md` (if applicable)
- Issue: #123 (if applicable)

## Changes

### [Category: e.g., Core, TUI, Docs]
- Change with brief explanation
- Another change

## Type
- [ ] Bug fix | [ ] Feature | [ ] Breaking change | [ ] Refactor | [ ] Docs

## Testing
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --all-targets` passes
- [ ] Manual: [what was tested]

## Notes
[Design decisions, trade-offs, future work — if relevant]
```

**Commit format**: `<type>: <subject>` where type is `feat|fix|docs|refactor|perf|test|chore`

---

**Start**: Analyze branch diff and generate the PR description.
