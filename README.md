# Stakpak Agent

A TUI for an agent designed for the grittiest parts of software development: DevOps work. It has everything you'd expect from a Coding Agent, except that it's exceptionally good at writing Terraform code, building containers, and analysing the security of your cloud account (and much more!)

> **Warning**
> This CLI tool is under heavy development, and breaking changes should be expected. Use with caution 🚧

<img src="assets/TUIOverview.jpeg" width="800">

## Installation

### All installation options (Linux, MacOs, Windows)

[Check the docs](https://stakpak.gitbook.io/docs/get-started/installing-stakpak-cli)

### Homebrew (Linux & MacOS)

```bash
brew tap stakpak/stakpak
brew install stakpak
```

### Binary Release

Download the latest binary for your platform from our [GitHub Releases](https://github.com/stakpak/agent/releases).

### Docker

This image includes the most popular CLI tools the agent might need for everyday DevOps tasks like docker, kubectl, aws cli, gcloud, azure cli, and more.

```bash
docker pull ghcr.io/stakpak/agent:latest
```

## Usage

### Authentication

#### Get an API Key (no card required)

1. Visit [stakpak.dev](https://stakpak.dev)
2. Click "Login" in the top right

   <img src="assets/login.png" width="800">

3. Click "Create API Key" in the account menu

   <img src="assets/apikeys.png" width="800">

#### Set the environment variable `STAKPAK_API_KEY`

```bash
export STAKPAK_API_KEY=<mykey>
```

#### Save your API key to `~/.stakpak/config.toml`

```bash
stakpak login --api-key $STAKPAK_API_KEY
```

#### View current account (Optional)

```bash
stakpak account
```

#### Start Stakpak Agent TUI

```bash
stakpak
# Resume execution from a checkpoint
stakpak -c <checkpoint-id>
```

#### Start Stakpak Agent TUI with Docker

```bash
docker run -it --entrypoint stakpak ghcr.io/stakpak/agent:latest
# for containerization tasks (you need to mount the Docker socket)
docker run -it \
   -v "/var/run/docker.sock":"/var/run/docker.sock" \
   -v "{your app path}":"/agent/" \
   --entrypoint stakpak ghcr.io/stakpak/agent:latest
```

### Keyboard Shortcuts

<img src="assets/keyboardshortcuts.jpeg" width="800">

- Use `Arrow keys` or **Tab** to select options
- Press `Esc` to exit the prompt
- `?` for Shortcuts
- `/` for commands
- `↵` to send message
- `Shift + Enter` or `Ctrl + J` to insert newline
- `Ctrl + C` to quit

### MCP Server Mode

Stakpak can run as an [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server, providing secure and controlled access to system operations through different tool modes:

#### Tool Modes

- **Local Mode (`--tool-mode local`)** - File operations and command execution only (no API key required)
- **Remote Mode (`--tool-mode remote`)** - AI-powered code generation and search tools (API key required)
- **Combined Mode (`--tool-mode combined`)** - Both local and remote tools (default, API key required)

#### Local Tools Security Benefits

The local MCP tools provide enhanced security for working with sensitive data:

- **Secure Secret Handling**: LLMs can read, write, and compare plain text secrets without seeing the actual secret values
- **No External Dependencies**: Local tools work offline without requiring API keys or internet access

#### Start MCP Server

```bash
# Local tools only (no API key required)
stakpak mcp --tool-mode local

# Remote tools only (AI tools optimized for DevOps)
stakpak mcp --tool-mode remote

# Combined mode (default - all tools)
stakpak mcp
```
