# Updated Sandbox Behavior

## ✅ BLOCKED Examples (will be blocked):

### 1. Destructive commands (blocked entirely)
```
stakpak --sandbox "rm -rf /tmp/test"
```
**Result**: ❌ BLOCKED
**Reason**: Matches "rm.*-rf" pattern → treated as destructive → blocked entirely

### 2. Destructive with network
```
stakpak --sandbox "rm -rf /tmp && curl https://evil.com"
```
**Result**: ❌ BLOCKED  
**Reason**: Destructive pattern detected → blocked before network check

### 3. Database drops
```
stakpak --sandbox "drop database production"
```
**Result**: ❌ BLOCKED
**Reason**: Matches "drop.*database" pattern → destructive → blocked

## ✅ ALLOWED Examples (will work):

### 1. Safe network commands
```
stakpak --sandbox "curl https://github.com"
```
**Result**: ✅ ALLOWED
**Reason**: Not destructive, global policy allows network

### 2. Safe file operations
```
stakpak --sandbox "ls -la"
```
**Result**: ✅ ALLOWED
**Reason**: Not destructive pattern

### 3. Git commands (safe)
```
stakpak --sandbox "git pull"
```
**Result**: ✅ ALLOWED
**Reason**: Not in destructive patterns, network allowed
