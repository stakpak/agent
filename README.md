# Stakpak Agent CLI
A CLI for the Stakpak API. Manage all your DevOps flows and configurations in one place, with AI-agents helping you out.

> **Warning**
> This CLI tool is under heavy development and breaking changes should be expected. Use with caution 🚧

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

Download the latest binary for your platform from our [GitHub Releases](https://github.com/stakpak/cli/releases).

### Docker
This image includes the most popular CLI tools the agent might need for everyday DevOps tasks like docker, kubectl, aws cli, gcloud, azure cli, and more.
```bash
docker pull ghcr.io/stakpak/cli:latest
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
docker run -it --entrypoint stakpak ghcr.io/stakpak/cli:latest
# for containerization tasks (you need to mount the Docker socket)
docker run -it \
   -v "/var/run/docker.sock":"/var/run/docker.sock" \
   -v "{your app path}":"/agent/" \
   --entrypoint stakpak ghcr.io/stakpak/cli:latest
```

## Keyboard Shortcuts
<img src="assets/keyboardshortcuts.jpeg" width="800">

- Use `Arrow keys` or **Tab** to select options  
- Press `Esc` to exit the prompt
- `?` for Shortcuts  
- `/` for commands  
- `↵` to send message  
- `Shift + Enter` or `Ctrl + J` to insert newline  
- `Ctrl + C` to quit

