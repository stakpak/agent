# Sandbox Testing Examples

## Command Examples to Test:

### 1. Safe Network Command
```bash
stakpak --sandbox "What's the IP of github.com?"
# LLM will likely run: `curl github.com`
# Expected: Should log network access in audit log
```

### 2. Destructive Command
```bash
stakpak --sandbox "Delete all files in /tmp/test"
# LLM will likely run: `rm -rf /tmp/test/...`
# Expected: Network should be BLOCKED (policy: rm.*-rf blocks network)
```

### 3. Git Command
```bash
stakpak --sandbox "Pull latest changes from main branch"
# LLM will likely run: `git pull origin main`
# Expected: Should work normally (not in destructive list)
```

### 4. Database Destructive
```bash
stakpak --sandbox "Drop the users table from database"
# LLM will likely run: `psql -c "DROP TABLE users"`
# Expected: Network BLOCKED (policy: drop.*database blocks network)
```

## Expected Behavior (Once Integrated):

1. **Audit Logging**: Commands logged to `~/.stakpak/sandbox-audit.log`
2. **Network Control**: Destructive commands blocked from network access
3. **Policy Enforcement**: Pattern matching on command strings
4. **Kernel Restrictions**: File system and syscall restrictions (future)

## Current Behavior (Not Integrated Yet):

- All commands run normally (no restrictions)
- No audit logging occurs
- Network access always allowed
