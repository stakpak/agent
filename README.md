# Stakpak Agent

**The most secure agent built for operations & DevOps.** Designed for the grittiest parts of software development with enterprise-grade security features including mutual TLS (mTLS) encryption, dynamic secret redaction, and privacy-first architecture.

<img src="assets/TUIOverview.png" width="800">

## 🔒 Security Hardened

- **Mutual TLS (mTLS)** - End-to-end encrypted communication between agent components
- **Dynamic Secret Substitution** - AI can read/write/compare secrets without seeing actual values
- **Secure Password Generation** - Generate cryptographically secure passwords with configurable complexity
- **Privacy Mode** - Redacts sensitive data like IP addresses and AWS account IDs

## 🛠️ Built for DevOps Work

- **Asynchronous Task Management** - Run background commands like port forwarding and servers with proper tracking and cancellation
- **Real-time Progress Streaming** - Long-running processes (Docker builds, deployments) stream progress updates in real-time
- **Infrastructure Code Indexing** - Automatic local indexing and semantic search for Terraform, Kubernetes, Dockerfile, and GitHub Actions
- **Documentation Research Agent** - Built-in web search for technical documentation, cloud providers, and development frameworks
- **Subagents** - Specialized research agents for code exploration and sandboxed analysis with different tool access levels (enabled with `--enable-subagents` flag)
- **Bulk Message Approval** - Approve multiple tool calls at once for efficient workflow execution
- **Reversible File Operations** - All file modifications are automatically backed up with recovery capabilities

## 🧠 Adaptive Intelligence

- **Rule Books** - Customize agent behavior with internal standard operating procedures, playbooks, and organizational policies
- **Persistent Knowledge** - Agent learns from interactions, remembers incidents, resources, and environment details to adapt to your workflow

## Installation

### All installation options (Linux, MacOs, Windows)

[Check the docs](https://stakpak.gitbook.io/docs/get-started/installing-stakpak-cli)

### Homebrew (Linux & MacOS)

```bash
brew tap stakpak/stakpak
brew install stakpak
```

To update it you can use

```bash
brew update
brew upgrade stakpak
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

#### Browser Compatibility Note

**Brave Browser Users**: Brave's security features may block automatic redirects to localhost ports during the API key creation flow. If you encounter this:

- **Option 1**: Click the security shield icon in the address bar and allow the redirect
- **Option 2**: Wait ~15 seconds for the timeout, then manually add your API key using the steps below

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

#### Start MCP Server

```bash
# Local tools only (no API key required, mTLS enabled by default)
stakpak mcp --tool-mode local

# Remote tools only (AI tools optimized for DevOps)
stakpak mcp --tool-mode remote

# Combined mode (default - all tools with full security)
stakpak mcp

# Disable mTLS (NOT recommended for production)
stakpak mcp --disable-mcp-mtls
```

### Agent Client Protocol (ACP)

ACP is a standardized protocol that enables AI agents to integrate directly with code editors like Zed, providing seamless AI-powered development assistance.

#### What ACP Offers with Stakpak

- **Real-time AI Chat** - Natural language conversations with context-aware AI assistance
- **Live Code Analysis** - AI can read, understand, and modify your codebase in real-time
- **Tool Execution** - AI can run commands, edit files, search code, and perform development tasks
- **Session Persistence** - Maintains conversation context across editor sessions
- **Streaming Responses** - Real-time AI responses with live progress updates
- **Agent Plans** - Visual task breakdown and progress tracking

#### Installation & Setup

1. **Install Stakpak** (if not already installed)
2. **Configure Zed Editor** - Add to `~/.config/zed/settings.json`:

```json
{
  "agent_servers": {
    "Stakpak": {
      "command": "stakpak",
      "args": ["acp"],
      "env": {}
    }
  }
}
```

3. **Start ACP Agent**:

```bash
stakpak acp
```

4. **Use in Zed** - Click Assistant (✨) → `+` → `New stakpak thread`

### Rulebook Management

Manage your standard operating procedures (SOPs), playbooks, and runbooks with Stakpak Rulebooks. Rulebooks customize agent behavior and provide context-specific guidance.

```bash
# List all rulebooks
stakpak rulebooks get
# or use the short alias
stakpak rb get

# Get a specific rulebook
stakpak rb get stakpak://my-org/deployment-guide.md

# Create or update a rulebook from a markdown file
stakpak rb apply my-rulebook.md

# Delete a rulebook
stakpak rb delete stakpak://my-org/old-guide.md
```

#### Rulebook Format

Rulebooks are markdown files with YAML frontmatter:

```markdown
---
uri: stakpak://my-org/deployment-guide.md
description: Standard deployment procedures for production
tags:
  - deployment
  - production
  - sop
---

# Deployment Guide

Your deployment procedures and guidelines here...
```

## Platform Testing

### Windows

Comprehensive testing report for Windows CLI functionality, including installation, configuration, and integration with WSL2 and Docker.

[View Windows Testing Report](platform-testing/windows-testing-report.md)

---

## ⭐ Like what we're building?

If our Agent saves you time or makes your DevOps life easier,  
**consider giving us a star on GitHub — it really helps!**

## [![Star on GitHub](https://img.shields.io/github/stars/stakpak/agent?style=social)](https://github.com/stakpak/agent/stargazers)
