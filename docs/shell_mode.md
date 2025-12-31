# Shell Mode

Stakpak's Shell Mode allows you to execute system commands directly from the TUI, seamlessly blending AI assistance with traditional terminal workflows.

## How to Activate

- **Empty Input:** Type `$` as the **first character** in the input bar.
- **Shortcuts:** Press `Shift + 4` (on US keyboards) when the input is empty.

Once activated, the prompt changes to `$`, indicating you are in Shell Mode.

## Background vs. Foreground Shell

Stakpak intelligently truncates long-running commands or those requiring user interaction.

### Background Shell
For standard commands (e.g., `ls`, `git status`, `docker ps`), Stakpak runs them in the background.
- **Non-blocking:** You can continue to use the TUI while the command runs.
- **Output:** The output is displayed in the main chat interface as a block.

### Foreground Shell (Interactive)
For interactive commands (e.g., `vim`, `htop`, `ssh`, `git commit`), Stakpak launches a full interactive terminal session when you `ctrl+r`.
- **Full Control:** You have complete control over the terminal process.
- **Pty Support:** Uses a pseudo-terminal (pty) to support full-screen applications and complex interactions.
- **Exit:** When the command exits, you are returned to the Stakpak TUI.

## Key Features
- **History:** Shell history is preserved for the session.
- **AI Context:** Output from shell commands is visible to the AI, allowing you to ask questions about the result of a command immediately after running it.
