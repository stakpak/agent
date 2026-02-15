## How to Update

### Predefined Commands (Embedded)
**Location:** `libs/api/src/slash_commands/*.md`

To update predefined commands:
1. Edit the `.md` files in `libs/api/src/slash_commands/`
2. Add new commands to `PREDEFINED_COMMANDS` in `predefined.rs`
3. Rebuild and release

### Personal Commands
**Location:** `~/.stakpak/commands/cmd_*.md`

Users can create personal commands that work across all projects:
```bash
mkdir -p ~/.stakpak/commands
echo "# My Command\nDo something..." > ~/.stakpak/commands/cmd_mycommand.md
# Available as /cmd:mycommand
```

### Project Commands
**Location:** `./.stakpak/commands/cmd_*.md`

Project-specific commands that override personal commands:
```bash
mkdir -p .stakpak/commands
echo "# Deploy\nDeploy to staging..." > .stakpak/commands/cmd_deploy.md
# Available as /cmd:deploy
```

### Config Definitions (Highest Priority)
**Location:** `~/.stakpak/config.toml`

Override any command with explicit file paths:
```toml
[profiles.default.commands.definitions]
security-review = "~/my-prompts/custom-security.md"
```
