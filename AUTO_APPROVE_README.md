# Auto-Approve System Documentation

## Overview

The Auto-Approve system is a sophisticated tool call approval mechanism that allows users to configure automatic approval policies for different tools and commands. It provides intelligent risk assessment, customizable policies, and a focus-based user interface for managing tool call confirmations.

## Features

### 🎯 **Smart Risk Assessment**
- **Automatic command analysis** for dangerous patterns
- **Four risk levels**: Low, Medium, High, Critical
- **Pattern-based classification** for system operations
- **Intelligent fallback** to safe defaults

### 🔧 **Flexible Policy System**
- **Per-tool configuration** with individual policies
- **Global default policy** for unknown tools
- **Four policy types**: Auto, Prompt, Smart, Never
- **Persistent configuration** across sessions

### 🎮 **Focus-Based UI**
- **Tab to toggle** between Chat view and Dialog focus
- **Visual indicators** for focus state
- **Keyboard navigation** for dialog options
- **Context-aware scrolling** and navigation

### 🛡️ **Safety Mechanisms**
- **Fail-safe defaults** for unknown commands
- **Emergency override** options
- **Audit trail** of auto-approved actions
- **Graceful error handling**

## Configuration

### Configuration File Location

The auto-approve configuration is stored in:
- **Local**: `.stakpak/session/auto_approve.json` (current working directory)
- **Global**: `$HOME/.stakpak/session/auto_approve.json` (fallback)

### Configuration Structure

```json
{
  "enabled": true,
  "default_policy": "prompt",
  "tools": {
    "view": "auto",
    "create": "prompt",
    "str_replace": "prompt",
    "generate_password": "auto",
    "generate_code": "prompt",
    "search_docs": "auto",
    "search_memory": "auto",
    "read_rulebook": "auto",
    "local_code_search": "auto",
    "run_command": "smart",
    "run_command_async": "smart",
    "get_all_tasks": "auto",
    "cancel_async_task": "prompt",
    "get_task_details": "auto"
  },
  "command_patterns": {
    "safe_readonly": [
      "ls", "cat", "grep", "find", "pwd", "whoami",
      "echo", "head", "tail", "wc", "sort", "uniq"
    ],
    "sensitive_destructive": [
      "rm", "mv", "cp", "chmod", "chown", "sudo", "su",
      "dd", "mkfs", "fdisk", "format", "ssh-keygen",
      "gpg", "openssl", "certutil", "keytool"
    ],
    "interactive_required": [
      "ssh", "scp", "rsync", "vim", "nano", "less",
      "more", "top", "htop", "man", "info"
    ]
  }
}
```

## Policy Types

### 🔓 **Auto**
- **Behavior**: Always auto-approve without prompting
- **Use case**: Safe, read-only operations
- **Examples**: `view`, `search_docs`, `get_all_tasks`

### ❓ **Prompt**
- **Behavior**: Always require user confirmation
- **Use case**: Potentially risky operations
- **Examples**: `create`, `str_replace`, `cancel_async_task`

### 🧠 **Smart**
- **Behavior**: Auto-approve safe commands, prompt for risky ones
- **Use case**: Commands that need intelligent assessment
- **Examples**: `run_command`, `run_command_async`

### 🚫 **Never**
- **Behavior**: Always block (not currently used in defaults)
- **Use case**: Highly sensitive operations
- **Examples**: Reserved for future use

## Risk Assessment

### 🟢 **Low Risk**
- **Criteria**: Read-only operations, information gathering
- **Auto-approval**: Yes (for Smart policy)
- **Examples**: `ls`, `cat`, `grep`, `find`, `pwd`, `whoami`

### 🟡 **Medium Risk**
- **Criteria**: File operations outside current directory
- **Auto-approval**: No
- **Examples**: Operations with `../`, `/home/`, `/tmp/`

### 🟠 **High Risk**
- **Criteria**: System modifications, privilege escalation
- **Auto-approval**: No
- **Examples**: `/etc/`, `/bin/`, `systemctl`, `service`

### 🔴 **Critical Risk**
- **Criteria**: Destructive operations, key generation
- **Auto-approval**: No
- **Examples**: `rm`, `sudo`, `ssh-keygen`, `gpg`, `openssl`

## User Interface

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+O` | Toggle auto-approve on/off |
| `Ctrl+Shift+O` | Auto-approve current tool |
| `Tab` | Toggle focus between Chat view and Dialog |
| `Up/Down` | Navigate dialog options (when dialog focused) |
| `Enter` | Select current dialog option |
| `Esc` | Cancel dialog and reject tool call |

### Focus System

#### **Chat View Focused**
- **Border**: Dialog border is dark gray
- **Navigation**: Up/Down arrows scroll messages
- **Hint**: "Press Tab to focus Dialog. Chat view focused"

#### **Dialog Focused**
- **Border**: Dialog border is yellow
- **Navigation**: Up/Down arrows navigate dialog options
- **Hint**: "Press Tab to focus Chat view. Dialog focused"

### Dialog Options

When a tool call requires confirmation, users see three options:

1. **Yes** - Accept the tool call once
2. **Yes, and don't ask again for [tool] commands** - Accept and set auto-approve policy
3. **No, and tell Stakpak what to do differently** - Reject and provide feedback

## Tool Classification

### Local Tools
- **view**: Auto (read-only file viewing)
- **create**: Prompt (file creation)
- **str_replace**: Prompt (file modification)
- **generate_password**: Auto (safe generation)

### Remote Tools
- **generate_code**: Prompt (code generation)
- **search_docs**: Auto (information retrieval)
- **search_memory**: Auto (information retrieval)
- **read_rulebook**: Auto (information retrieval)
- **local_code_search**: Auto (information retrieval)

### Command Execution Tools
- **run_command**: Smart (intelligent assessment)
- **run_command_async**: Smart (intelligent assessment)
- **get_all_tasks**: Auto (information retrieval)
- **cancel_async_task**: Prompt (task management)
- **get_task_details**: Auto (information retrieval)

## Safety Features

### Fail-Safe Defaults
- **Unknown commands**: Default to Prompt policy
- **Configuration errors**: Fall back to Prompt policy
- **Network issues**: Default to local-only approval
- **Permission errors**: Block auto-approval

### User Override Options
- **Emergency disable**: `Ctrl+A` to toggle off
- **Temporary disable**: Available through UI
- **Per-session override**: Configurable per session
- **Audit logging**: Track all auto-approved actions

### Security Boundaries
- **Never auto-approve**: `sudo`, `su`, `doas`
- **Always prompt for**: Operations outside project directory
- **Block auto-approval**: System file modifications
- **Require confirmation**: Network operations to unknown hosts

## Error Handling

### Configuration Validation
- **JSON schema validation** on config load
- **Graceful degradation** for invalid tool names
- **Warning messages** for deprecated options
- **Automatic config repair** for common issues

### Runtime Error Handling
- **Fallback to manual approval** on classification errors
- **Clear error messages** for configuration problems
- **Option to reset** corrupted configuration
- **Backup and restore** configuration capability

## Implementation Details

### Core Components

#### **AutoApproveManager**
- **Configuration management**: Loading, saving, validation
- **Policy resolution**: Tool-specific and default policies
- **Risk assessment**: Command pattern matching
- **State management**: Enabled/disabled status

#### **Risk Assessment Engine**
- **Pattern matching**: Against predefined command patterns
- **Risk level classification**: Low, Medium, High, Critical
- **Smart policy logic**: Auto-approve safe commands only

#### **Focus Management System**
- **State tracking**: Chat view vs Dialog focus
- **Visual indicators**: Border colors and status messages
- **Navigation routing**: Keyboard event handling

### File Structure

```
tui/src/
├── auto_approve.rs          # Core auto-approve logic
├── app.rs                   # AppState with focus management
├── event.rs                 # Keyboard event mapping
├── services/
│   ├── update.rs           # Event handling and state updates
│   ├── confirmation_dialog.rs  # Enhanced dialog UI
│   └── hint_helper.rs      # Focus status display
```

## Best Practices

### Configuration Management
1. **Start with Prompt policy** for new tools
2. **Gradually enable Auto** for trusted tools
3. **Use Smart policy** for command execution tools
4. **Regularly review** auto-approve settings

### Security Considerations
1. **Never auto-approve** system modification commands
2. **Always review** new tool policies
3. **Monitor audit logs** for unusual patterns
4. **Use Smart policy** for potentially risky operations

### User Experience
1. **Use focus system** to navigate efficiently
2. **Leverage keyboard shortcuts** for quick actions
3. **Review risk levels** before changing policies
4. **Provide feedback** when rejecting tool calls

## Troubleshooting

### Common Issues

#### **Auto-approve not working**
- Check if auto-approve is enabled (`Ctrl+A`)
- Verify tool policy in configuration
- Check risk level classification
- Review configuration file permissions

#### **Dialog not responding to keys**
- Ensure dialog is focused (yellow border)
- Use Tab to switch focus if needed
- Check if dialog is open (`is_dialog_open`)

#### **Configuration not saving**
- Verify file permissions on config directory
- Check JSON syntax in configuration file
- Ensure sufficient disk space
- Review error messages in logs

### Debug Information

Enable debug output by adding debug prints:
```rust
eprintln!("Dialog selected: {}, is_dialog_open: {}, dialog_command: {:?}", 
    state.dialog_selected, state.is_dialog_open, state.dialog_command.is_some());
```

## Future Enhancements

### Planned Features
- **Settings panel**: Visual configuration interface
- **Custom patterns**: User-defined command patterns
- **Configuration profiles**: Import/export settings
- **Advanced analytics**: Usage patterns and insights

### Potential Improvements
- **Machine learning**: Adaptive risk assessment
- **Integration**: External security tools
- **Audit dashboard**: Comprehensive logging interface
- **Policy templates**: Predefined security profiles

## Contributing

When contributing to the auto-approve system:

1. **Follow security-first approach** for new features
2. **Add comprehensive tests** for risk assessment logic
3. **Update documentation** for configuration changes
4. **Consider backward compatibility** for existing configs
5. **Review risk classifications** for new command patterns

---

*This documentation covers the comprehensive auto-approve system implementation. For specific implementation details, refer to the source code in `tui/src/auto_approve.rs`.* 