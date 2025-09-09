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

Stakpak implements the [Agent Client Protocol (ACP)](https://github.com/zed-industries/agent-client-protocol) to enable seamless integration with code editors like Zed. ACP standardizes communication between AI agents and development environments, providing a unified interface for AI-powered coding assistance.

#### Features Implemented

- **Chat Completion** - Natural language conversations with the AI agent for code generation, refactoring, and debugging
- **Session Management** - Persistent sessions with checkpoint support for resuming conversations
- **Streaming Responses** - Real-time streaming of AI responses for better user experience

#### Integration with Zed Editor

To use Stakpak as an ACP agent in Zed, you need to configure it in Zed's settings:

##### Through Zed UI
1. **Open Zed** and click the Assistant (✨) icon in the bottom right corner
2. **Access Settings** in the Agent Panel (top right corner)
3. Add the following configuration to your Zed settings file (`~/.config/zed/settings.json`):

```json
{
  "agent_servers": {
    "stakpak": {
      "command": "stakpak",
      "args": ["acp"],
      "env": {}
    }
  }
}
```
4. Click on the `+` icon on the top right corner and choose `New stakpak thread` under `External Agents`
Once configured, you can:
- Engage in natural language conversations with the AI agent
- Generate code snippets and refactor existing code
- Receive explanations for complex code segments
- Resume previous conversations using checkpoint IDs

#### Start ACP Agent

```bash
# Start ACP agent (requires API key)
stakpak acp
```

#### MCP Tool Calls Status
IN_PROGRESS

For more information about ACP integration, visit the [Zed Documentation on Agent Panel](https://zed.dev/docs/ai/agent-panel) and [Model Context Protocol Documentation](https://zed.dev/docs/ai/mcp).

---

## ⭐ Like what we're building?

If our Agent saves you time or makes your DevOps life easier,  
**consider giving us a star on GitHub — it really helps!**

## [![Star on GitHub](https://img.shields.io/github/stars/stakpak/agent?style=social)](https://github.com/stakpak/agent/stargazers)
