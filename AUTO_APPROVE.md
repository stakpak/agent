# Auto-Approve System Documentation

## Overview

The Auto-Approve system is a tool call approval mechanism that allows users to configure automatic approval policies for different tools. It provides intelligent risk assessment, customizable policies, and a user interface for managing tool call confirmations.

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

### 🛡️ **Safety Mechanisms**
- **Fail-safe defaults** for unknown commands
- **Emergency override** options
- **Audit trail** of auto-approved actions
- **Graceful error handling**

## Configuration

### Configuration File Location

The auto-approve configuration is stored in:
- **Local**: `.stakpak/session/auto_approve.json` (current working directory)

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
    "run_command": "prompt",
    "run_command_async": "prompt",
    "get_all_tasks": "auto",
    "cancel_async_task": "prompt",
    "get_task_details": "auto"
  },
  "command_patterns": {
    "safe_readonly": [],
    "sensitive_destructive": [],
    "interactive_required": []
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
- **Examples**: `create`, `str_replace`, `run_command`

### 🧠 **Smart**
- **Behavior**: Auto-approve safe commands, prompt for risky ones
- **Use case**: Commands that need intelligent assessment
- **Examples**: `run_command` (when command patterns are configured)

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
- **Examples**: Operations with `../`, `/home/`, `/tmp/` - Not implemented yet

### 🟠 **High Risk**
- **Criteria**: System modifications, privilege escalation
- **Auto-approval**: No
- **Examples**: `/etc/`, `/bin/`, `systemctl`, `service` - Not implemented yet

### 🔴 **Critical Risk**
- **Criteria**: Destructive operations, key generation
- **Auto-approval**: No
- **Examples**: `rm`, `sudo`, `ssh-keygen`, `gpg`, `openssl` - Not implemented yet

## User Interface

### Keyboard Shortcuts

| Shortcut | Action | When to Use |
|----------|--------|-------------|
| `Ctrl+O` | Toggle auto-approve on/off | Anytime - enables/disables the entire auto-approve system |
| `Ctrl+Y` | Shows which tools are auto-approved |

### Command Interface

#### **Toggle Auto-Approve for Specific Tool**
- Command: `/toggle_auto_approve <tool_name>`
- Example: `/toggle_auto_approve view`
- Effect: Toggles the policy for the specified tool between Auto and Prompt

#### **View Auto-Approved Tools**
- Press `Ctrl+Y` when no dialog is open
- Shows a list of tools currently set to auto-approve

## Tool Classification

### Auto-Approved Tools (Default)
- **view**: Auto (read-only file viewing)
- **generate_password**: Auto (safe generation)
- **search_docs**: Auto (information retrieval)
- **search_memory**: Auto (information retrieval)
- **read_rulebook**: Auto (information retrieval)
- **local_code_search**: Auto (information retrieval)
- **get_all_tasks**: Auto (information retrieval)
- **get_task_details**: Auto (information retrieval)

### Prompt-Required Tools (Default)
- **create**: Prompt (file creation)
- **str_replace**: Prompt (file modification)
- **generate_code**: Prompt (code generation)
- **run_command**: Prompt (command execution)
- **run_command_async**: Prompt (async command execution)
- **cancel_async_task**: Prompt (task management)

### Smart Policy Tools
- **run_command**: Smart (when command patterns are configured)
- **run_command_async**: Smart (when command patterns are configured)

## Safety Features

### Fail-Safe Defaults
- **Unknown commands**: Default to Prompt policy
- **Configuration errors**: Fall back to Prompt policy
- **Network issues**: Default to local-only approval
- **Permission errors**: Block auto-approval

### Security Boundaries
- **Never auto-approve**: `sudo`, `su`, `doas`
- **Always prompt for**: Operations outside project directory
- **Block auto-approval**: System file modifications
- **Require confirmation**: Network operations to unknown hosts

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

### File Structure

```
tui/src/
├── services/
│   ├── auto_approve.rs       # Core auto-approve logic
│   ├── update.rs             # Event handling and state updates
│   └── hint_helper.rs        # Status display
├── app.rs                    # AppState with auto-approve manager
└── event.rs                  # Keyboard event mapping
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
1. **Use keyboard shortcuts** for quick actions
2. **Review risk levels** before changing policies
3. **Provide feedback** when rejecting tool calls
4. **Check auto-approve status** regularly

## Troubleshooting

### Common Issues

#### **Auto-approve not working**
- Check if auto-approve is enabled (`Ctrl+O`)
- Verify tool policy in configuration
- Check risk level classification
- Review configuration file permissions

#### **Configuration not saving**
- Verify file permissions on config directory
- Check JSON syntax in configuration file
- Ensure sufficient disk space
- Review error messages in logs

### Debug Information

The system provides fallback behavior when configuration loading fails:
- Creates default configuration if none exists
- Falls back to Prompt policy for unknown tools
- Logs errors to stderr for debugging

## Current Limitations

### No Commands Supported Initially
- The system starts with no commands configured for auto-approve
- Users must manually configure which tools to auto-approve
- Default policy is "Prompt" for all tools

### Configuration Management
- Configuration is stored locally in `.stakpak/session/auto_approve.json`
- No global configuration override
- Manual configuration required for each project

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

*This documentation covers the current auto-approve system implementation. For specific implementation details, refer to the source code in `tui/src/services/auto_approve.rs`.* 