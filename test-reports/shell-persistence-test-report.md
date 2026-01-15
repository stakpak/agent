# Shell Persistence Test Report

**Date:** 2026-01-15  
**Branch:** `feat/persistent-shell-sessions`  
**Test Environment:** macOS with Docker SSH server (`rastasheep/ubuntu-sshd:18.04`)

---

## Overview

This report documents comprehensive testing of shell session persistence across all command execution modes in Stakpak:

- **Local Foreground** - `run_command` without remote parameter
- **Remote Foreground** - `run_command` with SSH remote connection
- **Local Background** - `run_command_task` without remote parameter
- **Remote Background** - `run_command_task` with SSH remote connection

## Test Setup

```bash
# SSH server container for remote testing
docker run -d --name ssh-test-server -p 2222:22 rastasheep/ubuntu-sshd:18.04
# Credentials: root/root
```

---

## Reproduction Guide

Follow these steps to reproduce all tests manually:

### 1. Start SSH Test Server

```bash
# Start container
docker run -d --name ssh-test-server -p 2222:22 rastasheep/ubuntu-sshd:18.04

# Verify it's running
docker ps | grep ssh-test-server

# Test SSH connectivity (password: root)
ssh -o StrictHostKeyChecking=no -p 2222 root@localhost echo "SSH works"
```

### 2. Test Local Foreground Persistence

Run these commands sequentially in Stakpak using `run_command`:

```bash
# Command 1: Set state
export LOCAL_VAR="test123" && cd /tmp && my_func() { echo "arg: $1"; }

# Command 2: Verify persistence
echo "VAR=$LOCAL_VAR CWD=$(pwd)" && my_func "hello"
# Expected: VAR=test123 CWD=/tmp
#           arg: hello
```

### 3. Test Remote Foreground Persistence

Run these commands with `run_command` using `remote: root@localhost:2222` and `password: root`:

```bash
# Command 1: Set state
export REMOTE_VAR="remote456" && cd /tmp && alias ll='ls -la'

# Command 2: Verify persistence
echo "VAR=$REMOTE_VAR CWD=$(pwd)" && alias ll
# Expected: VAR=remote456 CWD=/tmp
#           alias ll='ls -la'
```

### 4. Test Background Task Isolation

Run these using `run_command_task`:

```bash
# Background Task 1: Set variable
export BG_VAR="background789" && echo "Set: $BG_VAR"
# Expected output: Set: background789

# Background Task 2: Try to read Task 1's variable
echo "Read: $BG_VAR"
# Expected output: Read:  (empty - isolated)
```

Then verify foreground can't see background state using `run_command`:

```bash
echo "Foreground sees: $BG_VAR"
# Expected: Foreground sees:  (empty - isolated)
```

### 5. Test Remote Background Isolation

Run with `run_command_task` using `remote: root@localhost:2222` and `password: root`:

```bash
# Remote Background: Set and echo
export REMOTE_BG="remotebg" && echo "VAR=$REMOTE_BG CWD=$(pwd)"
# Expected: VAR= CWD=/root (variable doesn't expand, starts in /root)
```

### 6. Cleanup

```bash
docker rm -f ssh-test-server
```

### Quick Validation Script

Save this as `test-persistence.sh` and run sections manually:

```bash
#!/bin/bash
# This is a reference - run commands through Stakpak, not directly

echo "=== LOCAL FOREGROUND ==="
# run_command: export TEST=1 && cd /tmp
# run_command: echo "TEST=$TEST CWD=$(pwd)"
# Expected: TEST=1 CWD=/tmp

echo "=== REMOTE FOREGROUND ==="
# run_command (remote): export TEST=2 && cd /var
# run_command (remote): echo "TEST=$TEST CWD=$(pwd)"
# Expected: TEST=2 CWD=/var

echo "=== LOCAL BACKGROUND ==="
# run_command_task: export BG=3 && echo $BG
# run_command: echo "BG=$BG"
# Expected: BG= (empty)

echo "=== REMOTE BACKGROUND ==="
# run_command_task (remote): export RBG=4 && echo $RBG
# run_command (remote): echo "RBG=$RBG"
# Expected: RBG= (empty)
```

---

## Results

### Environment Variables

| Mode | Set Variable | Persists Across Commands | Notes |
|------|-------------|-------------------------|-------|
| **Local Foreground** | ✓ | ✓ | Full persistence within session |
| **Remote Foreground** | ✓ | ✓ | Full persistence within session |
| **Local Background** | ✓ | ✗ | Isolated - doesn't persist to foreground |
| **Remote Background** | ✗ | ✗ | Doesn't even set properly (runs in fresh shell) |

**Test Commands:**
```bash
# Local Foreground - SET
export LOCAL_TEST_VAR="local_value_123" && echo "Set: $LOCAL_TEST_VAR"
# Output: Set: local_value_123

# Local Foreground - CHECK (subsequent command)
echo "Check: $LOCAL_TEST_VAR"
# Output: Check: local_value_123  ✓ PERSISTED

# Remote Foreground - SET
export REMOTE_TEST_VAR="remote_value_456" && echo "Set: $REMOTE_TEST_VAR"
# Output: Set: remote_value_456

# Remote Foreground - CHECK (subsequent command)
echo "Check: $REMOTE_TEST_VAR"
# Output: Check: remote_value_456  ✓ PERSISTED

# Local Background - SET
export BG_LOCAL_VAR="bg_local_value_789" && echo "Set: $BG_LOCAL_VAR"
# Output: Set: bg_local_value_789

# Local Foreground - CHECK BG VAR
echo "Check BG var: $BG_LOCAL_VAR"
# Output: Check BG var:  ✗ NOT PERSISTED
```

---

### Working Directory (CWD)

| Mode | Change CWD | Persists Across Commands | Notes |
|------|-----------|-------------------------|-------|
| **Local Foreground** | ✓ | ✓ | `cd /tmp` persisted |
| **Remote Foreground** | ✓ | ✓ | `cd /tmp` persisted |
| **Local Background** | ✓ | ✗ | CWD change isolated to task |
| **Remote Background** | ✗ | ✗ | Always starts in `/root` |

**Test Commands:**
```bash
# Local Foreground - CHANGE
cd /tmp && echo "CWD: $(pwd)"
# Output: CWD: /tmp

# Local Foreground - CHECK (subsequent command)
echo "CWD: $(pwd)"
# Output: CWD: /tmp  ✓ PERSISTED

# Remote Foreground - CHANGE
cd /tmp && echo "CWD: $(pwd)"
# Output: CWD: /tmp

# Remote Foreground - CHECK (subsequent command)
echo "CWD: $(pwd)"
# Output: CWD: /tmp  ✓ PERSISTED

# Local Background - CHANGE
cd /var && echo "CWD: $(pwd)"
# Output: CWD: /var

# Local Foreground - CHECK
echo "CWD: $(pwd)"
# Output: CWD: /tmp  ✗ BG change NOT visible (still /tmp from foreground)
```

---

### Shell Functions & Aliases

| Mode | Define | Persists Across Commands | Notes |
|------|--------|-------------------------|-------|
| **Local Foreground** | ✓ | ✓ | Functions and aliases persist |
| **Remote Foreground** | ✓ | ✓ | Functions and aliases persist |
| **Local Background** | N/A | N/A | Isolated - not practical to test |
| **Remote Background** | N/A | N/A | Isolated - not practical to test |

**Test Commands:**
```bash
# Local Foreground - DEFINE
my_func() { echo "Function called with: $1"; } && alias ll='ls -la'

# Local Foreground - CHECK (subsequent command)
my_func "test_arg" && alias ll
# Output: Function called with: test_arg
#         ll='ls -la'  ✓ BOTH PERSISTED

# Remote Foreground - DEFINE
my_remote_func() { echo "Remote function: $1"; } && alias rll='ls -la'

# Remote Foreground - CHECK (subsequent command)
my_remote_func "test_arg" && alias rll
# Output: Remote function: test_arg
#         alias rll='ls -la'  ✓ BOTH PERSISTED
```

---

### Shell History

| Mode | History Available | Persists |
|------|------------------|----------|
| **Local Foreground** | ✓ | ✓ |
| **Remote Foreground** | ✓ | ✓ |
| **Background (both)** | N/A | N/A |

**Test Output:**
```bash
# Local Foreground
history | tail -5
# Shows previous commands from session  ✓

# Remote Foreground
history | tail -5
# Shows previous commands from remote session  ✓
```

---

### Cross-Session Isolation

| Test | Result | Details |
|------|--------|---------|
| BG task 1 → BG task 2 (local) | ✗ Isolated | BG2 cannot see BG1's variables |
| BG task → Foreground (local) | ✗ Isolated | Foreground cannot see BG variables |
| BG task → Foreground (remote) | ✗ Isolated | Foreground cannot see BG variables |
| Subshell → Parent shell | ✗ Isolated | Expected POSIX behavior |

**Test Commands:**
```bash
# Background Task 1
export BG_SESSION_VAR="bg_session_123" && echo "BG1: $BG_SESSION_VAR"
# Output: BG1: bg_session_123

# Background Task 2 (started immediately after)
echo "BG2: checking BG1 var=$BG_SESSION_VAR"
# Output: BG2: checking BG1 var=  ✗ ISOLATED

# Subshell test (expected behavior)
(export SUBSHELL_VAR="val" && cd /usr && echo "In: $SUBSHELL_VAR $(pwd)")
echo "After: $SUBSHELL_VAR $(pwd)"
# Output: In: val /usr
#         After:  /tmp  ✗ ISOLATED (correct POSIX behavior)
```

---

### Special Cases

| Feature | Local FG | Remote FG | Local BG | Remote BG |
|---------|----------|-----------|----------|-----------|
| Exported vars | ✓ Persist | ✓ Persist | ✗ | ✗ |
| Non-exported vars | ✓ Persist | ✓ Persist | ✗ | ✗ |
| Special characters | ✓ | ✓ | ✓ | ✓ |
| Multi-line commands | ✓ | ✓ | ✓ | ✓ |

**Exported vs Non-Exported Test:**
```bash
# Set both types
NON_EXPORTED_VAR="not_exported" && export EXPORTED_VAR="is_exported"

# Check persistence (subsequent command)
echo "NON_EXPORTED: $NON_EXPORTED_VAR, EXPORTED: $EXPORTED_VAR"
# Output: NON_EXPORTED: not_exported, EXPORTED: is_exported
# ✓ BOTH persist in foreground sessions
```

**Special Characters Test:**
```bash
echo "Testing 'quotes' and \"double quotes\" and \$dollar and spaces   here"
# Output: Testing 'quotes' and "double quotes" and $dollar and spaces   here
# ✓ All special characters handled correctly
```

---

## Summary Table

| Execution Mode | Persistent Shell Session | Use Case |
|----------------|-------------------------|----------|
| **Local Foreground** (`run_command`) | ✓ **YES** | Interactive work, stateful operations |
| **Remote Foreground** (`run_command` + remote) | ✓ **YES** | Remote server management |
| **Local Background** (`run_command_task`) | ✗ **NO** | Long-running isolated tasks |
| **Remote Background** (`run_command_task` + remote) | ✗ **NO** | Remote long-running tasks |

---

## Key Findings

### ✓ What Works

1. **Foreground sessions are fully persistent** - Environment variables, working directory, functions, aliases, and history all persist across commands
2. **Local and remote foreground behave identically** - Both maintain persistent shell sessions
3. **Special characters and quoting work correctly** - No escaping issues observed

### ✗ What Doesn't Work

1. **Background tasks are completely isolated** - Each task runs in a fresh shell
2. **Background → Foreground state transfer** - Variables set in background tasks are not visible in foreground
3. **Background → Background state transfer** - Background tasks cannot share state with each other
4. **Remote background variable expansion** - Variables set in the same command don't expand properly (non-interactive shell)

---

## Recommendations

### For Users

1. **Use foreground commands for stateful workflows** - When you need to set variables, change directories, or define functions that subsequent commands will use
2. **Use background tasks for isolated long-running operations** - Port forwarding, log tailing, servers - where state isolation is acceptable or desired
3. **Pass all required context in a single background command** - Don't rely on state from previous commands

### For Developers

1. **Background tasks should document isolation behavior** - Users should understand that each task is independent
2. **Consider adding session-based background tasks** - Option to run background tasks in the same session as foreground (if needed)
3. **Remote background could benefit from PTY allocation** - Would fix variable expansion issues

---

## Test Environment Details

- **Host OS:** macOS (Apple Silicon)
- **Docker Image:** `rastasheep/ubuntu-sshd:18.04` (amd64 via emulation)
- **SSH Port:** 2222
- **Credentials:** root/root
- **Shell:** bash (remote), zsh (local)
