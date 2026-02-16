# Code Review

Review changes in this branch vs `origin/main`. Read full file context, not just diffs.

## Gather Changes

```bash
git fetch origin
git diff origin/main...HEAD --stat
git diff origin/main...HEAD
git log origin/main..HEAD --oneline
```

## Review Focus

**Correctness**
- Edge cases: empty inputs, null/none, errors
- No off-by-one in slicing/indexing
- Proper resource cleanup
- Concurrency and threading safety

**Security**
- No secrets in code or logs
- Input validation present
- No path traversal or injection risks

**Quality**
- Functions reasonably sized, single responsibility
- No duplication (DRY)
- Actionable error messages with context

**Language-specific** (adapt to the project)
- Idiomatic patterns for the language
- Avoid unsafe shortcuts (e.g. unwrap/expect in Rust, bare except in Python)
- Prefer standard library and common patterns

## Report Format

For each issue:

```
### [SEVERITY] `path/file` (line X-Y)
**Issue**: [description]
**Code**: [problematic snippet]
**Fix**: [suggested code]
**Why**: [explanation]
```

**Severity**: 🔴 CRITICAL (security/panics) | 🟠 MAJOR (bugs/logic) | 🟡 MINOR (quality) | 🔵 NIT (style)

## Output

1. **Summary table**: files reviewed, issue counts by severity
2. **Issues list**: grouped by severity, actionable
3. **What's good**: positive observations
4. **Offer fixes**: ask which issues to auto-fix

---

**Start**: Fetch diff, review each changed file in full context, report findings.
