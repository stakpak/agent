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
- RFCs or design docs being implemented
- `CONTRIBUTING.md` or similar for conventions
- Issue tracker for related tickets

## Output Format

```markdown
## Summary
[1-2 sentences: what this PR does and why]

## Related
- RFC: [link if applicable]
- Issue: #123 (if applicable)

## Changes

### [Category: e.g., Core, API, Docs]
- Change with brief explanation
- Another change

## Type
- [ ] Bug fix | [ ] Feature | [ ] Breaking change | [ ] Refactor | [ ] Docs

## Testing
- [ ] Tests pass
- [ ] Linter passes
- [ ] Manual: [what was tested]

## Notes
[Design decisions, trade-offs, future work — if relevant]
```

**Commit format** (Conventional Commits): `<type>: <subject>` — e.g. `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `chore`

---

**Start**: Analyze branch diff and generate the PR description.
