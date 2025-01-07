# Stakpak CLI

> **Warning**
> This CLI tool is under heavy development and breaking changes should be expected. Use with caution.

A CLI for the Stakpak API. Manage all your DevOps flows and configurations in one place, with AI-agents helping you out.

## Installation

### Homebrew (macOS & Linux)

```bash
brew tap stakpak/stakpak
brew install stakpak
```

### Binary Release

Download the latest binary for your platform from our [GitHub Releases](https://github.com/stakpak/cli/releases).

#### Linux (x86_64)

```bash
curl -L "https://github.com/stakpak/cli/releases/v0.1.22/download/stakpak-linux-x86_64.tar.gz" | tar xz
sudo mv stakpak /usr/local/bin/
```

#### macOS (Intel)

```bash
curl -L "https://github.com/stakpak/cli/releases/v0.1.22/download/stakpak-darwin-x86_64.tar.gz" | tar xz
sudo mv stakpak /usr/local/bin/
```

#### macOS (Apple Silicon)

```bash
curl -L "https://github.com/stakpak/cli/releases/v0.1.22/download/stakpak-darwin-aarch64.tar.gz" | tar xz
sudo mv stakpak /usr/local/bin/
```

### Docker

```bash
docker pull ghcr.io/stakpak/cli:latest
```

To run the CLI using Docker:

```bash
docker run ghcr.io/stakpak/cli:latest <command>
```

## Usage

### Authentication

#### Create an API Key

1. Visit [stakpak.ai](https://stakpak.ai)
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

### Flow management

- List flows
- Get flow versions
- Clone configurations from a flow version
- Push configurations to a new flow
- Push configurations to an existing flow
- Perform LLM-powered queries on your configurations

### Agents

- List agent types
- List agent sessions and checkpoints
- Get agent checkpoint state
- Run agent
- Run agent form a specific checkpoint
