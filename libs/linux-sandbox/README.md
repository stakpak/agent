# Linux-Sandbox

Kernel-level sandboxing for Stakpak using Landlock and seccomp.

## Features

- **Kernel-level restrictions** using Landlock (filesystem) and seccomp (syscalls)
- **Network control** - Allow or block network access based on policies
- **Policy-based execution** - TOML-based policy files
- **Audit logging** - JSON-based audit trail of all operations
- **Graceful degradation** - Works even without full kernel support

## Design

This sandbox works at the **kernel level** to provide security restrictions:

1. **Landlock** - Filesystem access control (Linux 5.13+)
2. **Seccomp** - System call filtering
3. **Network control** - Block network syscalls when policy dictates
4. **Audit logging** - All security decisions logged

## Policy Format

```toml
[sandbox]
mode = "readonly"  # readonly | workspace-write | full-access

[network]
allow_network = true
log_network = true

[[network.command_rules]]
pattern = "rm.*-rf"     # Regex pattern
allow_network = false   # Block network for this command
destructive = true

[[network.command_rules]]
pattern = "git pull"
allow_network = true
destructive = false

[audit]
enabled = true
log_file = "~/.stakpak/sandbox-audit.log"
log_level = "info"
log_file_access = true
log_network = true
log_commands = true
log_security_blocks = true
```

## Usage

```rust
use linux_sandbox::{Sandbox, SandboxPolicy};

// Load policy
let policy = SandboxPolicy::from_file("policy.toml")?;

// Create sandbox
let sandbox = Sandbox::new(policy);

// Execute command
let status = sandbox.execute_command("git", &["pull"])?;
```

## Architecture

```
┌─────────────────────────────────────┐
│   Sandbox (Parent Process)           │
│   ┌───────────────────────────────┐  │
│   │  1. Load Policy              │  │
│   │  2. Setup Network Controller │  │
│   │  3. Setup Audit Logger        │  │
│   │  4. Fork Child                │  │
│   └───────────────────────────────┘  │
│             ↓ fork()                 │
└──────────────────────────────────────┘
              │
┌─────────────▼─────────────┐
│  Sandboxed Process         │
│  ┌─────────────────────┐   │
│  │  1. Apply Landlock  │   │
│  │  2. Apply Seccomp   │   │
│  │  3. Execute Command │   │
│  └─────────────────────┘   │
└────────────────────────────┘
```

## Limitations

- **Network control is binary**: All network or no network (can't filter by domain)
- **Requires kernel support**: Landlock needs Linux 5.13+
- **Needs root for full functionality**: Some features require privileges

## Future Enhancements

- Add Landlock implementation
- Add seccomp filter implementation
- Add network inspection via MITM proxy
- Add Cedar policy engine integration

