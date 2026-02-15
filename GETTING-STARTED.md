# Getting Started with Stakpak CLI

**The most secure agent built for operations & DevOps.** Stakpak CLI is a powerful, security-hardened tool designed for the grittiest parts of software development with enterprise-grade security features.

## 🚀 Quick Start

### 1. Installation

Choose your preferred installation method:

#### Homebrew (Recommended for macOS/Linux)
```bash
brew tap stakpak/stakpak
brew install stakpak
```

#### Binary Release
Download the latest binary for your platform from [GitHub Releases](https://github.com/stakpak/agent/releases).

#### Docker
```bash
# Basic usage
docker pull ghcr.io/stakpak/agent:latest

# For containerization tasks (mount Docker socket)
docker run -it \
   -v "/var/run/docker.sock":"/var/run/docker.sock" \
   -v "{your app path}":"/agent/" \
   --entrypoint stakpak ghcr.io/stakpak/agent:latest
```

#### From Source (Development)
```bash
git clone https://github.com/stakpak/agent.git
cd agent
cargo build --release
```

### 2. Authentication

#### Get Your API Key
1. Visit [stakpak.dev](https://stakpak.dev)
2. Click "Login" → "Create API Key" (no card required)

#### Configure Authentication
```bash
# Option 1: Environment variable
export STAKPAK_API_KEY=<your-api-key>

# Option 2: Save to config file
stakpak login --api-key $STAKPAK_API_KEY

# Verify your account
stakpak account
```

### 3. First Run

```bash
# Start the interactive TUI
stakpak

# Or run a single command
stakpak --async "Help me understand this codebase"
```

### 4. Autopilot (24/7 Mode)

```bash
# One-time setup (channels + schedules + runtime defaults)
stakpak onboard

# Start autonomous runtime
stakpak up

# Stop autonomous runtime
stakpak down
```

Canonical subcommands are also available:

```bash
stakpak autopilot init
stakpak autopilot up
stakpak autopilot status
stakpak autopilot logs
stakpak autopilot down
stakpak autopilot doctor
```

## 🎯 Operation Modes

Stakpak offers multiple operation modes to fit different workflows:

### Interactive TUI Mode (Default)
```bash
stakpak
```
- Full-featured terminal interface
- Real-time chat with AI agent
- Visual progress tracking
- Tool call approval interface

### Async Mode
```bash
stakpak --async "Deploy my application"
stakpak --print "Analyze this error log"
```
- Non-interactive execution
- Perfect for automation and scripting
- Configurable step limits

### MCP Server Mode
```bash
# Local tools only (no API key required)
stakpak mcp --tool-mode local

# Combined mode (recommended)
stakpak mcp --tool-mode combined

# With custom configuration
stakpak mcp --enable-slack-tools --privacy-mode
```
- Model Context Protocol server
- Integrates with AI coding assistants
- Secure tool access control

### ACP Mode (Editor Integration)
```bash
stakpak acp
```
- Agent Client Protocol for editor integration
- Real-time code analysis and modification
- Works with Zed editor and other ACP-compatible editors

## 🔒 Security Features

### Mutual TLS (mTLS)
- End-to-end encrypted communication
- Automatically generated certificates
- Enabled by default for all modes

### Secret Redaction
```bash
# Automatic secret detection and redaction
stakpak --privacy-mode

# Disable redaction (NOT recommended)
stakpak --disable-secret-redaction
```

### Privacy Mode
- Redacts IP addresses, AWS account IDs, and other sensitive data
- Perfect for sharing logs or screenshots

## 🛠️ Core Capabilities

### Infrastructure Code Indexing
- Automatic indexing of Terraform, Kubernetes, Dockerfile, and GitHub Actions
- Semantic search across your infrastructure code
- Real-time file watching and updates

### Subagents (Incoming)
```bash
stakpak --enable-subagents
```
- **ResearchAgent**: Fast code exploration and documentation lookup
- **SandboxResearchAgent**: Secure containerized analysis with command execution

### Configuration Management
```bash
# View current config
stakpak config show

# Generate sample config
stakpak config sample

# Set machine name
stakpak set --machine-name "my-dev-machine"
```

## 📋 Configuration

### Profile-Based Configuration
Stakpak supports multiple configuration profiles for different environments:

```toml
# ~/.stakpak/config.toml
[profiles.default]
api_key = "your_api_key_here"
allowed_tools = ["view", "search_docs", "create", "run_command"]

[profiles.production]
api_key = "prod_api_key_here"
allowed_tools = ["view", "search_docs"]  # Read-only for safety

[profiles.development]
api_key = "dev_api_key_here"
allowed_tools = ["view", "search_docs", "create", "str_replace", "run_command"]
```

### Key Configuration Options
- `allowed_tools`: Control which tools the agent can use
- `auto_approve`: Automatically approve specific tool calls
- `rulebooks`: Customize agent behavior with organizational policies
- `machine_name`: Device identification for multi-machine setups

## 🎮 Keyboard Shortcuts

### Interactive Mode
- `Arrow keys` / `Tab`: Navigate options
- `Esc`: Exit current prompt
- `?`: Show shortcuts help
- `/`: Access commands
- `Enter`: Send message
- `Shift + Enter` / `Ctrl + J`: Insert newline
- `Ctrl + C`: Quit application

## 🔧 Advanced Usage

### Checkpoint System
```bash
# Resume from a checkpoint
stakpak -c <checkpoint-id>

# Run with specific working directory
stakpak --workdir /path/to/project
```

### Tool Restrictions
```bash
# Allow only specific tools
stakpak --tool view --tool search_docs

# Use custom system prompt
stakpak --system-prompt-file ./my-prompt.txt
```

### Study Mode
```bash
stakpak --study-mode
```
Optimizes the agent for learning and educational purposes.

### Large Project Support
```bash
stakpak --index-big-project
```
Allows indexing of projects with more than 500 files.

## 🐳 Docker Integration

The Docker image includes popular DevOps tools:
- Docker CLI
- AWS CLI
- Google Cloud CLI
- Azure CLI
- DigitalOcean CLI
- Terraform
- kubectl
- And more...

### Containerized Usage
```bash
# Basic containerized agent
docker run -it ghcr.io/stakpak/agent:latest

# With volume mounts for your project
docker run -it \
   -v "$(pwd)":/agent \
   -v "/var/run/docker.sock":"/var/run/docker.sock" \
   ghcr.io/stakpak/agent:latest

# With cloud credentials
docker run -it \
   -v "$(pwd)":/agent \
   -v "$HOME/.aws":/home/agent/.aws:ro \
   -v "$HOME/.kube":/home/agent/.kube:ro \
   ghcr.io/stakpak/agent:latest
```

## 🚨 Warden Mode

Stakpak Warden provides additional security by running agents in isolated containers:

```bash
# Run with default warden configuration
stakpak warden

# Custom warden setup
stakpak warden --volume "./:/agent:ro" --env "DEBUG=true"
```

## 🔄 Updates

```bash
# Check for updates
stakpak update

# Auto-update is enabled by default
```

## 📚 Next Steps

1. **Explore the TUI**: Run `stakpak` and start chatting with the agent
2. **Try MCP Mode**: Set up integration with your preferred AI coding assistant
3. **Configure Profiles**: Set up different profiles for development and production
4. **Index Your Code**: Let Stakpak automatically index your infrastructure code
5. **Enable Subagents**: Experiment with specialized research agents

## 🆘 Getting Help

- **Documentation**: [stakpak.gitbook.io](https://stakpak.gitbook.io/docs)
- **Issues**: [GitHub Issues](https://github.com/stakpak/agent/issues)
- **Discussions**: [GitHub Discussions](https://github.com/stakpak/agent/discussions)
- **Website**: [stakpak.dev](https://stakpak.dev)

## 🎉 You're Ready!

Stakpak CLI is now installed and configured. Start with the interactive TUI (`stakpak`) to explore its capabilities, or dive into specific modes based on your workflow needs.

Remember: Stakpak is designed for security-first operations. All file modifications are automatically backed up, secrets are redacted by default, and communication is encrypted with mTLS.

Happy coding! 🚀
