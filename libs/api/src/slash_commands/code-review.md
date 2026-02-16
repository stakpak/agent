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

**Rust-Specific**
- No `unwrap()`/`expect()` outside tests — use `?`, `ok()`, `unwrap_or`
- No unnecessary `.clone()` — prefer references
- No locks held across `.await` points
- Iterators over manual loops

**Correctness**
- Edge cases: empty inputs, None, errors
- No off-by-one in slicing/indexing
- Proper resource cleanup (RAII)
- Concurrency safety

**Security**
- No secrets in code/logs
- Input validation present
- No path traversal or command injection

**Quality**
- Functions < 50 lines, single responsibility
- No duplication (DRY)
- Actionable error messages with context

## Report Format

For each issue:

```
### [SEVERITY] `path/file.rs` (line X-Y)
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
