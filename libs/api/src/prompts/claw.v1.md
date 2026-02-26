
# Deploy & Monitor OpenClaw Gateway with Stakpak Autopilot

## Goals

* Deploy a fully functional OpenClaw AI gateway with Docker on any target (EC2, VPS, or local)
* Connect Telegram as the primary messaging channel with DM pairing security
* Harden the deployment for production (fail-closed auth, loopback binding, OS-level security)
* Configure Stakpak Autopilot for continuous health monitoring with Telegram or Discord alerts
* Produce a deployment that any agent can reproduce from this skill alone

## Core Principles

* Collect ALL tokens and API keys before starting — every phase has blocking dependencies
* Never assume environment variables work for LLM auth — OpenClaw uses its own `auth-profiles.json` store
* Never share a Telegram bot token between OpenClaw and Stakpak — causes 409 Conflict polling errors
* Fix volume permissions (UID 1000) BEFORE first container start — avoids EACCES errors
* Write scripts locally and SCP to remote hosts — never create scripts via SSH heredoc (shell escaping breaks)
* Use `config set` for all OpenClaw configuration — many CLI subcommands that seem obvious do not exist
* Always verify with `channels status --probe` and `models status` after configuration changes

## Prerequisites

### Mandatory User Prompt: Deployment Target

> **ASK THE USER** before starting: "Where are you deploying OpenClaw?"
>
> | Target | Description | Phases to follow |
> |--------|-------------|-----------------|
> | **EC2** | Fresh AWS EC2 instance | Phase 1 (full) → 2 → 3 → 4 (optional) → 5 → 6 → 7 |
> | **Existing VPS** | Any Linux server with SSH access (Hetzner, DigitalOcean, etc.) | Phase 1 (skip to 1.7) → 2 → 3 → 4 (optional) → 5 → 6 → 7 |
> | **Local Docker** | Developer machine | Phase 2 → 3 → 5 (partial) → 6 (local checks only) → 7 |
>
> Store the user's choice as `$DEPLOY_TARGET` — used throughout.

### Mandatory User Prompt: Stakpak Alert Channel

> **ASK THE USER**: "Which channel for Stakpak autopilot alerts — Telegram or Discord?"
>
> * **Telegram** — requires a SECOND Telegram bot (separate from OpenClaw). Simpler if user already uses Telegram.
> * **Discord** — requires a Discord bot. Better if user wants separation between chat and monitoring.
>
> **CRITICAL**: If user picks Telegram, they MUST create a separate bot from the OpenClaw one.
> Two processes cannot poll the same Telegram bot token — causes 409 Conflict errors.

Store the user's choice as `$ALERT_CHANNEL` (`telegram` or `discord`).

### Required Tokens — Collect All Before Starting

| Token | Source | Format | Phase |
|-------|--------|--------|-------|
| Anthropic API key | https://console.anthropic.com/settings/keys | `sk-ant-api03-...` | 2 |
| **OpenClaw Telegram bot token** | Telegram → `@BotFather` → `/newbot` | `123456789:ABCdef...` | 3 |
| Telegram user ID | Telegram → `@userinfobot` → send any message | Numeric | 3 |
| **Stakpak alert channel token** | See alert channel choice above | Varies | 6 |
| AWS credentials (EC2 only) | IAM console or `aws configure` | Access key + secret | 1 |
| SSH access (VPS only) | Hosting provider | `user@host` + key | 1 |
| Domain name (optional) | DNS provider | FQDN | 4 |

### Telegram Bot Setup (BotFather)

This process is used TWICE if user picks Telegram for both OpenClaw and Stakpak alerts.
Each bot needs its own unique token.

1. Open Telegram, message `@BotFather`
2. Send `/newbot`
3. Choose a display name (e.g. "My OpenClaw" for the gateway, "Stakpak Alerts" for monitoring)
4. Choose a username (must end in `bot`)
5. Copy the token from BotFather's reply
6. Send `/setprivacy` → `Disable` (required for group messages without @mention)
7. Validate: `curl -s "https://api.telegram.org/bot<TOKEN>/getMe"` must return `"ok": true`
8. **Send the bot a message** (e.g. "hi") — this creates the chat so it can send you alerts

### Stakpak Autopilot LLM Provider

> **⚠ CRITICAL**: Before starting autopilot, verify that the Stakpak CLI has a working LLM provider.
> Run `stakpak auth list` to check. If empty, run `stakpak auth login` to configure one.
> Without this, every schedule will fail silently with `Provider not found`.

## OpenClaw Application Context

### What OpenClaw Is

OpenClaw is a **single-user** multi-channel AI assistant gateway. It routes messages from 20+ messaging platforms to an AI agent. It is NOT multi-tenant.

* **Docker image**: `ghcr.io/openclaw/openclaw` (multi-arch amd64+arm64)
* **Default port**: 18789 (WebSocket gateway), 18790 (bridge)
* **Database**: Embedded SQLite — no external DB, cache, or queue needed
* **Container user**: `node` (UID 1000)
* **Config file**: `/home/node/.openclaw/openclaw.json` (inside container)
* **Auth store**: `/home/node/.openclaw/agents/main/agent/auth-profiles.json` (inside container)
* **Sizing**: t3.small / 2 vCPU / 2GB RAM minimum

### CLI Commands That Exist

```
health --json          # Health check (exits non-zero on failure)
status --all / --deep  # Full status / probe gateway
doctor / doctor --fix  # Diagnostics
channels status --probe  # Per-channel connectivity
config set <key> <val> # Set config value
config get <key>       # Get config value
pairing list           # List pending pairing requests
pairing approve telegram <CODE>  # Approve a Telegram user
models status          # Show LLM auth status
models status --check  # Machine-readable exit codes: 0=ok, 1=expired, 2=expiring-soon
security audit --deep  # Security audit
cron status            # Cron scheduler status
cron list              # List cron jobs
cron runs --id <jobId> --limit N  # Job run history
cron run <jobId>       # Manually trigger a cron job
message send --channel telegram --target <ID> --message "text"
gateway status         # Service status (systemd/launchd)
sessions list          # List active sessions
sandbox explain        # Sandbox scope config
system heartbeat last  # Last heartbeat timestamp and skip reason
logs --follow          # Stream live logs
```

### LLM Auth Configuration

OpenClaw does NOT read LLM keys from environment variables. It uses its own auth store.

* **Auth store path** (inside container): `/home/node/.openclaw/agents/main/agent/auth-profiles.json`
* **Format**: JSON with `version: 1` and a `profiles` map keyed by `<provider>:<label>`
* **Supported providers**: `anthropic`, `openai`, `gemini`, `groq`, and others
* **Cooldown behavior**: When a provider returns rate-limit errors, the profile enters exponential backoff cooldown (1m → 5m → 25m → 1h cap). Billing failures (e.g. "insufficient credits") trigger longer disables (starts at 5h, doubles per failure, caps at 24h). During cooldown, the profile is silently skipped and the next profile/model in the fallback chain is used.
* **Verify**: `models status` — must show provider status as `ok`
* **Reference**: https://docs.openclaw.ai/gateway/authentication

> **⚠ Discovery note**: Before writing auth config, check the latest format with
> `docker exec openclaw-gateway node dist/index.js doctor` — it will report auth issues
> and suggest the correct format. The OpenClaw docs at the URL above are the authoritative source.

### Telegram Config Keys

Use `config set channels.telegram.<key>`. Reference: https://docs.openclaw.ai/channels/telegram

Key config values:

* `botToken` — bot token string
* `dmPolicy` — `pairing` | `allowlist` | `open` | `disabled`
* `groupPolicy` — `open` | `allowlist` | `disabled`
* `allowFrom` — JSON array of numeric Telegram user IDs
* `streamMode` — `off` | `partial` | `block`
* `mediaMaxMb` — integer
* `actions.sendMessage`, `actions.deleteMessage`, `actions.reactions` — boolean

> **⚠ Discovery note**: Config keys evolve between versions. If a key is rejected,
> check the configuration reference at https://docs.openclaw.ai/gateway/configuration-reference
> or run `docker exec openclaw-gateway node dist/index.js doctor` for diagnostics.

### Docker Volume Mapping

```
Host                             → Container
/opt/openclaw/config             → /home/node/.openclaw
/opt/openclaw/workspace          → /home/node/.openclaw/workspace
/opt/openclaw/data               → /data
```

Container runs as UID 1000. Host dirs MUST be `chown -R 1000:1000` before first start.

## Workflow

### Phase 1: Infrastructure Provisioning

> **Skip to Phase 1.2** if `$DEPLOY_TARGET` = "Existing VPS"
> **Skip to Phase 2** if `$DEPLOY_TARGET` = "Local Docker"

#### 1.1 Provision EC2 Instance (EC2 only)

Use the `simple-deployment-on-vm` skill or your standard AWS VM provisioning workflow.

**Requirements for the EC2 instance:**
* Instance type: `t3.small` minimum (2 vCPU / 2GB RAM)
* OS: Amazon Linux 2023 (x86_64)
* EBS: 30GB gp3, encrypted
* Security group: SSH (port 22) from your IP only. Add 80/443 later if configuring TLS in Phase 4
* Elastic IP: assign one to avoid IP drift on stop/start
* IMDSv2: enforce `HttpTokens=required`

**User data bootstrap** — install on the instance after launch:
```bash
dnf update -y
dnf install -y docker git fail2ban yum-cron
systemctl enable docker && systemctl start docker
systemctl enable --now fail2ban
mkdir -p /usr/local/lib/docker/cli-plugins
curl -SL "https://github.com/docker/compose/releases/latest/download/docker-compose-linux-$(uname -m)" \
  -o /usr/local/lib/docker/cli-plugins/docker-compose
chmod +x /usr/local/lib/docker/cli-plugins/docker-compose
fallocate -l 2G /swapfile && chmod 600 /swapfile && mkswap /swapfile && swapon /swapfile
echo '/swapfile swap swap defaults 0 0' >> /etc/fstab
useradd -m -s /bin/bash openclaw
usermod -aG docker openclaw
mkdir -p /opt/openclaw/{config,workspace,data,checks,logs}
chown -R openclaw:openclaw /opt/openclaw
```

#### 1.2 Prepare Existing VPS

> For `$DEPLOY_TARGET` = "Existing VPS" — start here.

Write the bootstrap script locally, then SCP and execute (never use SSH heredoc — see Core Principles).

Create `/tmp/openclaw-vps-bootstrap.sh` locally:

```bash
#!/bin/bash
set -euo pipefail
apt-get update && apt-get install -y docker.io docker-compose-plugin fail2ban ufw unattended-upgrades || \
dnf install -y docker docker-compose-plugin fail2ban yum-cron

systemctl enable --now docker fail2ban

if ! swapon --show | grep -q /swapfile; then
  fallocate -l 2G /swapfile && chmod 600 /swapfile
  mkswap /swapfile && swapon /swapfile
  echo "/swapfile swap swap defaults 0 0" >> /etc/fstab
fi

useradd -m -s /bin/bash openclaw 2>/dev/null || true
usermod -aG docker openclaw
mkdir -p /opt/openclaw/{config,workspace,data,checks,logs}
chown -R openclaw:openclaw /opt/openclaw
```

Deploy and run:

```bash
scp -i $SSH_KEY /tmp/openclaw-vps-bootstrap.sh $SSH_USER@$PUBLIC_IP:/tmp/
ssh -i $SSH_KEY $SSH_USER@$PUBLIC_IP 'sudo bash /tmp/openclaw-vps-bootstrap.sh'
```

### Phase 2: Docker & OpenClaw Setup

#### 2.1 Fix Volume Permissions

```bash
# Remote:
ssh -i $SSH_KEY $SSH_USER@$PUBLIC_IP \
  'sudo chown -R 1000:1000 /opt/openclaw/config /opt/openclaw/workspace /opt/openclaw/data'

# Local:
mkdir -p ~/openclaw/{config,workspace,data}
```

#### 2.2 Generate Gateway Token

Write the token generation script locally, then SCP and execute:

Create `/tmp/openclaw-gen-token.sh` locally:

```bash
#!/bin/bash
set -euo pipefail
GATEWAY_TOKEN=$(openssl rand -hex 32)
echo "OPENCLAW_GATEWAY_TOKEN=$GATEWAY_TOKEN" > /opt/openclaw/.env
echo "$GATEWAY_TOKEN" > /opt/openclaw/config/gateway-token
chmod 600 /opt/openclaw/.env /opt/openclaw/config/gateway-token
```

```bash
# Remote:
scp -i $SSH_KEY /tmp/openclaw-gen-token.sh $SSH_USER@$PUBLIC_IP:/tmp/
ssh -i $SSH_KEY $SSH_USER@$PUBLIC_IP 'sudo -u openclaw bash /tmp/openclaw-gen-token.sh'

# Local:
GATEWAY_TOKEN=$(openssl rand -hex 32)
echo "OPENCLAW_GATEWAY_TOKEN=$GATEWAY_TOKEN" > ~/openclaw/.env
echo "$GATEWAY_TOKEN" > ~/openclaw/config/gateway-token
chmod 600 ~/openclaw/.env ~/openclaw/config/gateway-token
```

#### 2.3 Create Docker Compose File

```yaml
services:
  openclaw-gateway:
    image: ghcr.io/openclaw/openclaw:latest
    container_name: openclaw-gateway
    restart: unless-stopped
    ports:
      - "127.0.0.1:18789:18789"
      - "127.0.0.1:18790:18790"
    volumes:
      - /opt/openclaw/config:/home/node/.openclaw
      - /opt/openclaw/workspace:/home/node/.openclaw/workspace
      - /opt/openclaw/data:/data
    env_file:
      - /opt/openclaw/.env
    environment:
      - NODE_ENV=production
      - NODE_OPTIONS=--max-old-space-size=1536
      - OPENCLAW_GATEWAY_BIND=lan
      - OPENCLAW_GATEWAY_PORT=18789
    healthcheck:
      test: ["CMD", "node", "dist/index.js", "health", "--json"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 30s
    logging:
      driver: json-file
      options:
        max-size: "50m"
        max-file: "5"
```

#### 2.4 Pull Image & Start

```bash
docker pull ghcr.io/openclaw/openclaw:latest
docker compose up -d
# Wait 20s, then verify:
docker compose ps   # Must show (healthy)
```

#### 2.5 Set Gateway Mode & Write LLM Auth

```bash
docker exec openclaw-gateway node dist/index.js config set gateway.mode "local"
```

Write the LLM auth profile per https://docs.openclaw.ai/gateway/authentication or run `doctor` for guidance.

```bash
docker exec openclaw-gateway node dist/index.js models status
# Must show provider status as ok.
docker compose restart
```

### Phase 3: Telegram Integration

#### 3.1 Add Bot Token & Configure Security

```bash
docker exec openclaw-gateway node dist/index.js config set channels.telegram.botToken "<TELEGRAM_BOT_TOKEN>"
docker exec openclaw-gateway node dist/index.js config set channels.telegram.dmPolicy "pairing"
docker exec openclaw-gateway node dist/index.js config set channels.telegram.groupPolicy "allowlist"
docker exec openclaw-gateway node dist/index.js config set channels.telegram.mediaMaxMb 5
docker exec openclaw-gateway node dist/index.js config set channels.telegram.actions.sendMessage true
docker exec openclaw-gateway node dist/index.js config set channels.telegram.actions.reactions true
docker exec openclaw-gateway node dist/index.js config set channels.telegram.reactionLevel "minimal"
```

#### 3.2 Restart & Verify

```bash
docker compose restart
# Wait 15s:
docker exec openclaw-gateway node dist/index.js channels status --probe
# Must show: Telegram default: enabled, configured, running, mode:polling, works
```

#### 3.3 Approve First User

```bash
docker exec openclaw-gateway node dist/index.js pairing approve telegram <PAIRING_CODE>
# Or pre-approve by user ID:
docker exec openclaw-gateway node dist/index.js config set channels.telegram.allowFrom "[<USER_ID>]"
```

### Phase 4: TLS & Reverse Proxy (Optional)

> Skip if no domain name. Only for EC2/VPS targets.

```bash
# Install Caddy:
sudo dnf copr enable -y @caddy/caddy && sudo dnf install -y caddy   # RHEL/AL2023
# Or: sudo apt install -y caddy                                       # Debian/Ubuntu
```

Write `/etc/caddy/Caddyfile`:

```
<DOMAIN> {
    reverse_proxy 127.0.0.1:18789
    @websocket {
        header Connection *Upgrade*
        header Upgrade websocket
    }
    reverse_proxy @websocket 127.0.0.1:18789
    header {
        X-Content-Type-Options nosniff
        X-Frame-Options DENY
        Referrer-Policy strict-origin-when-cross-origin
        -Server
    }
}
```

```bash
sudo systemctl enable caddy && sudo systemctl start caddy
curl -I https://<DOMAIN>
```

### Phase 5: Security Hardening

> For local Docker deployments, only 5.1 and 5.2 apply.

#### 5.1 OpenClaw Application Security

```bash
docker exec openclaw-gateway node dist/index.js config set discovery.mdns.mode "off"
docker exec openclaw-gateway node dist/index.js config set logging.redactSensitive "tools"
docker exec openclaw-gateway node dist/index.js config set logging.file "/data/logs/openclaw.log"
```

#### 5.2 File Permissions

```bash
chmod 700 /opt/openclaw/config
chmod 600 /opt/openclaw/config/openclaw.json
chmod 600 /opt/openclaw/.env
chmod 600 /opt/openclaw/config/gateway-token
```

#### 5.3 SSH Hardening (EC2/VPS only)

```bash
sudo sed -i 's/^#*PermitRootLogin.*/PermitRootLogin no/' /etc/ssh/sshd_config
sudo sed -i 's/^#*MaxAuthTries.*/MaxAuthTries 3/' /etc/ssh/sshd_config
sudo systemctl restart sshd
```

#### 5.4 AWS Hardening (EC2 only)

* Enforce IMDSv2: `aws ec2 modify-instance-metadata-options --instance-id $INSTANCE_ID --http-tokens required --http-endpoint enabled --region $REGION`
* Ensure EBS is encrypted (should already be if provisioned with `--encrypted` in Phase 1.1). For existing unencrypted volumes, use the standard snapshot → encrypted copy → volume swap procedure.

#### 5.5 Security Checklist

* [ ] Gateway token set (32-byte hex, fail-closed)
* [ ] `auth-profiles.json` valid (`models status` shows `ok`)
* [ ] Port 18789 bound to 127.0.0.1 only
* [ ] File permissions: config dir = 700, openclaw.json = 600, .env = 600
* [ ] mDNS disabled, log redaction enabled
* [ ] Telegram DM policy = pairing, group policy = allowlist
* [ ] SSH key-only auth, root login disabled, MaxAuthTries = 3
* [ ] fail2ban installed and running
* [ ] Security group: only port 22 (your IP) — 80/443 only if Caddy configured
* [ ] Elastic IP assigned (EC2) — no IP drift on stop/start
* [ ] IMDSv2 enforced (EC2)
* [ ] EBS encrypted (EC2)
* [ ] 2GB swap configured
* [ ] Automatic OS updates enabled
* [ ] `gateway.mode` = `local`
* [ ] systemd linger enabled (VPS with systemd user service)
* [ ] Node runtime is system Node, not Bun or version-manager path

### Phase 6: Stakpak Autopilot Monitoring

Stakpak Autopilot runs on your LOCAL machine and monitors the deployment via SSH (remote) or directly (local).

#### 6.0 Install & Configure Stakpak CLI

##### Install Stakpak

```bash
# macOS (Homebrew):
brew tap stakpak/stakpak && brew install stakpak

# Linux / macOS (curl):
curl -sSL https://stakpak.dev/install.sh | sh

# Verify:
stakpak --version
```

> **Reference**: https://github.com/stakpak/agent for latest install instructions.

##### Configure LLM Provider

Autopilot needs an LLM provider to investigate failures. Without this, every schedule fails silently with `Provider not found`.

```bash
# Check if a provider is already configured:
stakpak auth list

# If empty — configure one:
stakpak auth login
```

The `auth login` flow will prompt you to select a provider (Anthropic, OpenAI, DeepSeek, etc.) and enter your API key. The key is stored locally in `~/.stakpak/`.

> **Tip**: DeepSeek or Qwen work great for autopilot investigation at a fraction of the cost. You don't need Opus for monitoring.

##### Configure Alert Channel

Autopilot needs a notification channel to alert you when checks fail. Without a channel, it runs checks and investigates silently — you'll never know something broke.

**Option A: Telegram** (recommended if you already use Telegram)

You need a **SECOND** Telegram bot — separate from the OpenClaw bot. Two processes cannot poll the same bot token (causes 409 Conflict).

1. Follow the BotFather setup in Prerequisites to create a new bot (e.g. "Stakpak Alerts")
2. **Send the bot a message first** (e.g. "hi") — this creates the chat
3. Get your chat ID:

```bash
curl -s "https://api.telegram.org/bot<STAKPAK_BOT_TOKEN>/getUpdates" | jq '.result[0].message.chat.id'
```

4. Add the channel to Stakpak:

```bash
stakpak autopilot channel add telegram --token <STAKPAK_TELEGRAM_BOT_TOKEN> --target <CHAT_ID>
```

> **⚠ CRITICAL**: The `--target <CHAT_ID>` flag is REQUIRED. Without it, autopilot runs silently (log: `missing gateway notifications config`). If you forgot `--target`, remove and re-add:
> ```bash
> stakpak autopilot channel remove telegram
> stakpak autopilot channel add telegram --token <TOKEN> --target <CHAT_ID>
> ```

**Option B: Discord**

1. Go to https://discord.com/developers/applications
2. Click "New Application" → name it (e.g. "Stakpak Alerts")
3. Go to "Bot" → "Add Bot" → copy the bot token
4. Go to "OAuth2" → "URL Generator" → select `bot` scope → select `Send Messages` permission
5. Copy the generated URL → open it → invite the bot to your server
6. Add the channel to Stakpak:

```bash
stakpak autopilot channel add discord --token <DISCORD_BOT_TOKEN>
```

##### Verify Channel

```bash
stakpak autopilot channel test
```

This sends a test message to your configured channel. If it fails:

| Symptom | Fix |
|---------|-----|
| `missing gateway notifications config` | You forgot `--target`. Re-add the channel with `--target <CHAT_ID>` |
| `401 Unauthorized` | Bot token is invalid or revoked. Regenerate at BotFather (Telegram) or Developer Portal (Discord) |
| `400 Bad Request` (Discord) | Bot token format is wrong, or bot wasn't invited to the server |
| `409 Conflict` (Telegram) | Another process is polling this bot token. Use a different bot |
| No message received | For Telegram: did you message the bot first? The chat must exist before Stakpak can send to it |

```bash
# Verify channel is configured:
stakpak autopilot channel list
```

##### Start Autopilot Daemon

```bash
# Start autopilot (runs in background, survives terminal close):
stakpak up --non-interactive

# Check status:
stakpak autopilot status
```

`stakpak up` starts the autopilot daemon that:
- Runs all scheduled checks on their cron intervals
- On check failure: triggers an LLM investigation (SSH into server, read logs, diagnose)
- Sends investigation results + recommended fix to your alert channel
- `--non-interactive` skips confirmation prompts (safe for scripts/automation)

> **If `stakpak up` fails**: check `stakpak auth list` (provider configured?) and `stakpak autopilot channel list` (channel configured?). Both are required.

#### 6.1 Check Scripts

Write each script to the deployment host. For remote targets, write locally then SCP — never create via SSH heredoc.

**health.sh** — Gateway process health:
```bash
#!/bin/bash
RESPONSE=$(docker exec openclaw-gateway node dist/index.js health --json 2>&1)
if [ $? -ne 0 ]; then
  echo "FAIL: Gateway health check failed"
  echo "Response: $RESPONSE"
  echo "Container: $(docker inspect --format='{{.State.Status}}' openclaw-gateway 2>&1)"
  exit 1
fi
echo "OK: Gateway healthy"
```

**service.sh** — Gateway service status (systemd/process):
```bash
#!/bin/bash
# For Docker deployments:
STATUS=$(docker inspect --format='{{.State.Status}}' openclaw-gateway 2>/dev/null)
HEALTH=$(docker inspect --format='{{.State.Health.Status}}' openclaw-gateway 2>/dev/null)
if [ "$STATUS" != "running" ]; then
  echo "FAIL: Container status=$STATUS"; exit 1
fi
if [ "$HEALTH" = "unhealthy" ]; then
  echo "FAIL: Container running but unhealthy"; exit 1
fi
# Check for restart loop
RESTARTS=$(docker inspect --format='{{.RestartCount}}' openclaw-gateway 2>/dev/null)
if [ "${RESTARTS:-0}" -gt 5 ]; then
  echo "WARN: Container has restarted $RESTARTS times"; exit 1
fi
echo "OK: Container $STATUS, health=$HEALTH, restarts=$RESTARTS"
```

**channels.sh** — Channel connectivity:
```bash
#!/bin/bash
RESULT=$(docker exec openclaw-gateway node dist/index.js channels status --probe 2>&1)
if [ $? -ne 0 ] || echo "$RESULT" | grep -qi "disconnected\|error\|failed\|loggedOut"; then
  echo "FAIL: Channel issue"; echo "$RESULT"; exit 1
fi
echo "OK: Channels healthy"
```

**models.sh** — LLM auth credentials:
```bash
#!/bin/bash
RESULT=$(docker exec openclaw-gateway node dist/index.js models status --check 2>&1)
EXIT=$?
if [ $EXIT -eq 1 ]; then
  echo "FAIL: Model credentials expired or missing"; echo "$RESULT"; exit 1
elif [ $EXIT -eq 2 ]; then
  echo "WARN: Model credentials expiring within 24h"; echo "$RESULT"; exit 1
fi
echo "OK: Model auth valid"
```

**auth-cooldown.sh** — Auth profile cooldown/billing detection:
```bash
#!/bin/bash
AUTH_FILE="/opt/openclaw/config/agents/main/agent/auth-profiles.json"
if [ ! -f "$AUTH_FILE" ]; then
  AUTH_FILE=$(docker exec openclaw-gateway find /home/node/.openclaw/agents -name "auth-profiles.json" 2>/dev/null | head -1)
fi
COOLDOWNS=$(docker exec openclaw-gateway cat /home/node/.openclaw/agents/main/agent/auth-profiles.json 2>/dev/null | grep -c "cooldownUntil\|disabledReason")
if [ "${COOLDOWNS:-0}" -gt 0 ]; then
  echo "WARN: $COOLDOWNS auth profiles in cooldown or disabled"; exit 1
fi
echo "OK: No auth profiles in cooldown"
```

**resources.sh** — Disk, memory, container health:
```bash
#!/bin/bash
ERRORS=0
DISK_PCT=$(df /opt/openclaw | tail -1 | awk '{print $5}' | tr -d '%')
[ "$DISK_PCT" -gt 85 ] && echo "FAIL: Disk ${DISK_PCT}%" && ERRORS=$((ERRORS+1))
FREE_MB=$(free -m | awk '/^Mem:/{print $7}')
[ "$FREE_MB" -lt 200 ] && echo "FAIL: Memory ${FREE_MB}MB" && ERRORS=$((ERRORS+1))
STATUS=$(docker inspect --format='{{.State.Health.Status}}' openclaw-gateway 2>/dev/null)
[ "$STATUS" != "healthy" ] && echo "FAIL: Container $STATUS" && ERRORS=$((ERRORS+1))
# Check Docker daemon
docker info > /dev/null 2>&1 || { echo "FAIL: Docker daemon unreachable"; ERRORS=$((ERRORS+1)); }
[ $ERRORS -gt 0 ] && exit 1
echo "OK: disk=${DISK_PCT}%, mem=${FREE_MB}MB, container=$STATUS"
```

**cron-status.sh** — Cron scheduler health:
```bash
#!/bin/bash
STATUS_OUT=$(docker exec openclaw-gateway node dist/index.js cron status 2>&1)
if echo "$STATUS_OUT" | grep -qi "disabled\|error"; then
  echo "FAIL: Cron scheduler issue"; echo "$STATUS_OUT"; exit 1
fi
LIST_OUT=$(docker exec openclaw-gateway node dist/index.js cron list 2>&1)
FAILED=$(echo "$LIST_OUT" | grep -ci "failed\|error")
if [ "$FAILED" -gt 0 ]; then
  echo "WARN: $FAILED cron jobs with errors"; echo "$LIST_OUT"; exit 1
fi
echo "OK: Cron scheduler healthy"
```

**heartbeat.sh** — Heartbeat delivery:
```bash
#!/bin/bash
RESULT=$(docker exec openclaw-gateway node dist/index.js system heartbeat last 2>&1)
if echo "$RESULT" | grep -qi "requests-in-flight\|alerts-disabled\|unknown-accountId\|delivery.*fail"; then
  echo "WARN: Heartbeat issue"; echo "$RESULT"; exit 1
fi
echo "OK: Heartbeat delivering"
```

**queue.sh** — Message queue overflow:
```bash
#!/bin/bash
DROPS=$(docker logs openclaw-gateway --since 10m 2>&1 | grep -ci "drop\|overflow\|queued for")
if [ "$DROPS" -gt 3 ]; then
  echo "WARN: $DROPS queue drop/overflow events in last 10min"; exit 1
fi
echo "OK: Queue healthy"
```

**workspace-disk.sh** — Workspace disk growth:
```bash
#!/bin/bash
WORKSPACE_MB=$(du -sm /opt/openclaw/workspace/ 2>/dev/null | awk '{print $1}')
AGENTS_MB=$(du -sm /opt/openclaw/config/agents/ 2>/dev/null | awk '{print $1}')
TOTAL=$((${WORKSPACE_MB:-0} + ${AGENTS_MB:-0}))
if [ "$TOTAL" -gt 5120 ]; then
  echo "FAIL: OpenClaw data usage ${TOTAL}MB (>5GB)"; exit 1
fi
echo "OK: OpenClaw data usage ${TOTAL}MB"
```

**orphaned-sandbox.sh** — Orphaned sandbox containers:
```bash
#!/bin/bash
ORPHANS=$(docker ps -a --filter name=openclaw-sandbox --filter status=exited --format "{{.Names}}" | wc -l)
if [ "$ORPHANS" -gt 10 ]; then
  echo "WARN: $ORPHANS orphaned sandbox containers"; exit 1
fi
echo "OK: $ORPHANS orphaned containers"
```

**security-audit.sh** — Config permissions, DM policy, linger, runtime:
```bash
#!/bin/bash
ERRORS=0
PERMS=$(docker exec openclaw-gateway stat -c "%a" /home/node/.openclaw/openclaw.json 2>/dev/null)
[ "$PERMS" != "600" ] && echo "WARN: openclaw.json perms=$PERMS (should be 600)" && ERRORS=$((ERRORS+1))
DOCTOR=$(docker exec openclaw-gateway node dist/index.js doctor --non-interactive 2>&1)
echo "$DOCTOR" | grep -qi "open DM policy\|no allowlist" && echo "WARN: Open DM policy detected" && ERRORS=$((ERRORS+1))
echo "$DOCTOR" | grep -qi "bun\|nvm\|fnm\|volta\|asdf" && echo "WARN: Non-standard Node runtime" && ERRORS=$((ERRORS+1))
# Check linger (VPS only)
LINGER=$(loginctl show-user $(whoami) 2>/dev/null | grep -i linger)
[ "$LINGER" = "Linger=no" ] && echo "WARN: systemd linger disabled" && ERRORS=$((ERRORS+1))
[ $ERRORS -gt 0 ] && exit 1
echo "OK: Security audit passed"
```

**version.sh** — Version drift:
```bash
#!/bin/bash
RUNNING=$(docker exec openclaw-gateway node -e "console.log(require('./package.json').version)" 2>/dev/null)
LATEST=$(curl -sf "https://api.github.com/repos/openclaw/openclaw/releases/latest" \
  | grep -o '"tag_name":"[^"]*"' | cut -d'"' -f4 | sed 's/^v//')
[ -z "$RUNNING" ] || [ -z "$LATEST" ] && echo "WARN: versions unknown" && exit 0
[ "$RUNNING" != "$LATEST" ] && echo "UPDATE: $RUNNING → $LATEST" && exit 1
echo "OK: $RUNNING"
```

**compaction.sh** — Context compaction frequency:
```bash
#!/bin/bash
TODAY=$(date +%Y-%m-%d)
LOG_FILE="/opt/openclaw/data/logs/openclaw.log"
if [ ! -f "$LOG_FILE" ]; then
  LOG_FILE=$(docker logs openclaw-gateway --since 1h 2>&1)
  COUNT=$(echo "$LOG_FILE" | grep -ci "compaction complete\|auto-compact")
else
  COUNT=$(grep "$TODAY" "$LOG_FILE" 2>/dev/null | grep -ci "compaction complete\|auto-compact")
fi
if [ "${COUNT:-0}" -gt 10 ]; then
  echo "WARN: $COUNT compactions today (>10 threshold). Sessions burning tokens on context compaction."
  echo "Fix: enable agents.defaults.contextPruning.mode = cache-ttl"; exit 1
fi
echo "OK: $COUNT compactions today"
```

**presence.sh** — Connected clients snapshot (informational):
```bash
#!/bin/bash
# Informational audit check — presence tracks all WebSocket clients
# (CLI sessions, Web UI, macOS app, mobile nodes). Entries expire after
# 5 minutes of inactivity (TTL-based, max 200 entries).
RESULT=$(docker exec openclaw-gateway node dist/index.js status --all 2>&1)
echo "INFO: Connected clients snapshot"
echo "$RESULT" | grep -i "presence\|client\|connected" || echo "No presence data"
# Alert only if unexpected clients detected (more than expected count)
CLIENT_COUNT=$(echo "$RESULT" | grep -ci "connected\|client")
if [ "${CLIENT_COUNT:-0}" -gt 10 ]; then
  echo "WARN: Unusually high client count ($CLIENT_COUNT). Audit for unauthorized connections."
  echo "Fix: rotate gateway token — config set gateway.auth.token \"\$(openssl rand -hex 32)\""
  exit 1
fi
echo "OK"
```

**memory-search.sh** — Memory search index health:
```bash
#!/bin/bash
# Memory search builds a vector index over memory files for semantic recall.
# If the embedding provider API key expires, the local model path is missing,
# or QMD binary is not on PATH, memory search silently falls back to disabled.
RESULT=$(docker exec openclaw-gateway node dist/index.js status --all 2>&1)
if echo "$RESULT" | grep -qi "memory.*error\|memory.*disabled\|embedding.*fail\|qmd.*missing"; then
  echo "WARN: Memory search degraded or disabled"
  echo "$RESULT" | grep -i "memory"
  echo "Fix: check embedding provider key or set agents.defaults.memorySearch.provider openai"
  exit 1
fi
echo "OK: Memory search healthy"
```

**sandbox-image.sh** — Sandbox Docker image present:
```bash
#!/bin/bash
# When agent sandboxing is enabled (scope: "session"), the gateway requires
# the openclaw-sandbox:bookworm-slim image. If missing after Docker cleanup,
# every sandboxed tool call fails silently.
IMAGE=$(docker images openclaw-sandbox --format "{{.Repository}}:{{.Tag}}" 2>/dev/null | head -1)
if [ -z "$IMAGE" ]; then
  echo "FAIL: Sandbox image missing. Run scripts/sandbox-setup.sh"; exit 1
fi
echo "OK: Sandbox image present ($IMAGE)"
```

Deploy to remote host:

```bash
scp -i $SSH_KEY /tmp/openclaw-checks/*.sh $SSH_USER@$PUBLIC_IP:/tmp/
ssh -i $SSH_KEY $SSH_USER@$PUBLIC_IP 'sudo bash -c "
mv /tmp/*.sh /opt/openclaw/checks/
chown openclaw:openclaw /opt/openclaw/checks/*.sh
chmod +x /opt/openclaw/checks/*.sh
"'
```

> **Note**: Write each check script to `/tmp/openclaw-checks/` locally (e.g. `/tmp/openclaw-checks/health.sh`), then SCP the batch. Never create scripts via SSH heredoc.

#### 6.2 Create Local SSH Wrapper Scripts

```bash
mkdir -p ~/.stakpak/checks
for NAME in health service channels models auth-cooldown resources cron-status heartbeat queue workspace-disk orphaned-sandbox compaction presence memory-search sandbox-image security-audit version; do
  cat > ~/.stakpak/checks/openclaw-${NAME}.sh << EOF
#!/bin/bash
ssh -i $SSH_KEY -o StrictHostKeyChecking=no -o ConnectTimeout=10 \
  $SSH_USER@$PUBLIC_IP 'sudo -u openclaw /opt/openclaw/checks/${NAME}.sh'
EOF
  chmod +x ~/.stakpak/checks/openclaw-${NAME}.sh
done
```

#### 6.3 Verify Alert Channel

> If you haven't configured the alert channel yet, go back to **Phase 6.0**.

```bash
stakpak autopilot channel test
stakpak autopilot channel list
```

#### 6.4 Add Schedules

Every schedule MUST include `--channel $ALERT_CHANNEL`.

```bash
# CRITICAL — every 2-5 min
stakpak autopilot schedule add openclaw-health \
  --cron '*/5 * * * *' \
  --check ~/.stakpak/checks/openclaw-health.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 20 \
  --prompt "OpenClaw gateway health failed on $PUBLIC_IP. SSH in and investigate: docker logs openclaw-gateway --tail 100, docker ps. Restart if needed: cd /opt/openclaw && docker compose restart. Check: df -h && free -m."

stakpak autopilot schedule add openclaw-service \
  --cron '*/5 * * * *' \
  --check ~/.stakpak/checks/openclaw-service.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 15 \
  --prompt "OpenClaw container issue on $PUBLIC_IP. Check docker compose ps, docker logs --tail 100. If restart loop, check for OOM or stale lock: rm -f /data/gateway.*.lock."

stakpak autopilot schedule add openclaw-channels \
  --cron '*/5 * * * *' \
  --check ~/.stakpak/checks/openclaw-channels.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 15 \
  --prompt "OpenClaw channel probe failed on $PUBLIC_IP. Check: docker exec openclaw-gateway node dist/index.js channels status --probe. For Telegram loggedOut: re-add token. For WhatsApp: QR re-scan needed."

stakpak autopilot schedule add openclaw-models \
  --cron '*/15 * * * *' \
  --check ~/.stakpak/checks/openclaw-models.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "OpenClaw model auth failed on $PUBLIC_IP. Check: docker exec openclaw-gateway node dist/index.js models status --check. Exit 1=expired, 2=expiring. Update auth-profiles.json if needed."

# HIGH — every 30min-1hr
stakpak autopilot schedule add openclaw-auth-cooldown \
  --cron '*/30 * * * *' \
  --check ~/.stakpak/checks/openclaw-auth-cooldown.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "Auth profiles in cooldown or billing-disabled on $PUBLIC_IP. Check auth-profiles.json for cooldownUntil or disabledReason. Top up provider if billing issue."

stakpak autopilot schedule add openclaw-resources \
  --cron '0 */2 * * *' \
  --check ~/.stakpak/checks/openclaw-resources.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 15 \
  --prompt "Resource check failed on $PUBLIC_IP. Investigate disk, memory, container health, Docker daemon. Clean logs if full. Restart if unhealthy."

# MEDIUM — every 15min-6hr
stakpak autopilot schedule add openclaw-cron \
  --cron '*/15 * * * *' \
  --check ~/.stakpak/checks/openclaw-cron-status.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "OpenClaw cron issue on $PUBLIC_IP. Check: cron status, cron list. Run cron runs --id <jobId> for error details."

stakpak autopilot schedule add openclaw-heartbeat \
  --cron '*/30 * * * *' \
  --check ~/.stakpak/checks/openclaw-heartbeat.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "Heartbeat delivery issue on $PUBLIC_IP. Check system heartbeat last for skip reason. Verify channel connectivity and delivery target."

stakpak autopilot schedule add openclaw-queue \
  --cron '*/10 * * * *' \
  --check ~/.stakpak/checks/openclaw-queue.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "Message queue overflow on $PUBLIC_IP. Messages being dropped. Consider increasing messages.queue.cap or agents.defaults.maxConcurrent."

stakpak autopilot schedule add openclaw-workspace \
  --cron '0 */6 * * *' \
  --check ~/.stakpak/checks/openclaw-workspace-disk.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "Workspace disk usage high on $PUBLIC_IP. Check du -sh workspace/ agents/. Archive old session JSONL files: find sessions/ -name '*.jsonl' -mtime +30 | xargs gzip."

stakpak autopilot schedule add openclaw-sandbox \
  --cron '0 */1 * * *' \
  --check ~/.stakpak/checks/openclaw-orphaned-sandbox.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "Orphaned sandbox containers accumulating on $PUBLIC_IP. Clean: docker rm \$(docker ps -a --filter name=openclaw-sandbox --filter status=exited -q)."

stakpak autopilot schedule add openclaw-compaction \
  --cron '0 */1 * * *' \
  --check ~/.stakpak/checks/openclaw-compaction.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "Context compaction running >10 times/day on $PUBLIC_IP. Sessions burning tokens. Enable agents.defaults.contextPruning.mode = cache-ttl. Send /compact or /new in affected chats."

stakpak autopilot schedule add openclaw-sandbox-image \
  --cron '0 */6 * * *' \
  --check ~/.stakpak/checks/openclaw-sandbox-image.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "Sandbox Docker image missing on $PUBLIC_IP. Every sandboxed tool call is failing. Run scripts/sandbox-setup.sh to rebuild."

# LOW — daily/weekly
stakpak autopilot schedule add openclaw-security \
  --cron '0 9 * * *' \
  --check ~/.stakpak/checks/openclaw-security-audit.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "Security audit found issues on $PUBLIC_IP. Check config permissions, DM policy, linger, runtime. Run: doctor --non-interactive."

stakpak autopilot schedule add openclaw-version \
  --cron '0 9 * * 1' \
  --check ~/.stakpak/checks/openclaw-version.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "New OpenClaw version available on $PUBLIC_IP. Report current vs latest. Check CHANGELOG. Do NOT auto-update."

stakpak autopilot schedule add openclaw-presence \
  --cron '0 */1 * * *' \
  --check ~/.stakpak/checks/openclaw-presence.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 5 \
  --prompt "Unusually high client count on $PUBLIC_IP. Audit for unauthorized WebSocket connections. If unexpected, rotate gateway token: config set gateway.auth.token."

stakpak autopilot schedule add openclaw-memory-search \
  --cron '0 */6 * * *' \
  --check ~/.stakpak/checks/openclaw-memory-search.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 10 \
  --prompt "Memory search degraded or disabled on $PUBLIC_IP. Agent cannot recall past notes. Check embedding provider API key or QMD binary. Set agents.defaults.memorySearch.provider if needed."
```

#### 6.5 Start & Verify Autopilot

```bash
# If not already running from Phase 6.0:
stakpak up --non-interactive
stakpak autopilot status

# Dry-run a check to verify everything works end-to-end:
stakpak autopilot schedule trigger openclaw-health --dry-run
```

> Dry-run executes the check script, runs LLM investigation if it fails, but does NOT send a notification. Use it to verify checks work before going live.

#### 6.6 Complete Schedule Reference

| ID | Schedule | Cron | Tier | Purpose |
|----|----------|------|------|---------|
| C1 | openclaw-health | `*/5 * * * *` | Critical | Gateway process health (WS probe) |
| C2 | openclaw-service | `*/5 * * * *` | Critical | Container status, restart loops |
| C3 | openclaw-channels | `*/5 * * * *` | Critical | Channel auth — loggedOut, disconnected |
| C4 | openclaw-models | `*/15 * * * *` | Critical | LLM auth — expired/missing credentials |
| H1 | openclaw-auth-cooldown | `*/30 * * * *` | High | Auth profile cooldown / billing disabled |
| H2 | openclaw-resources | `0 */2 * * *` | High | Disk, memory, container, Docker daemon |
| H3 | openclaw-sandbox-image | `0 */6 * * *` | High | Sandbox Docker image present |
| M1 | openclaw-cron | `*/15 * * * *` | Medium | Cron scheduler disabled or job errors |
| M2 | openclaw-heartbeat | `*/30 * * * *` | Medium | Heartbeat delivery skipped |
| M3 | openclaw-queue | `*/10 * * * *` | Medium | Message queue overflow / drops |
| M4 | openclaw-workspace | `0 */6 * * *` | Medium | Workspace disk growth |
| M5 | openclaw-sandbox | `0 */1 * * *` | Medium | Orphaned sandbox containers |
| M6 | openclaw-compaction | `0 */1 * * *` | Medium | Context compaction frequency (>10/day) |
| L1 | openclaw-security | `0 9 * * *` | Low | Permissions, DM policy, linger, runtime |
| L2 | openclaw-version | `0 9 * * 1` | Low | Version drift |
| L3 | openclaw-presence | `0 */1 * * *` | Low | Connected clients snapshot (audit) |
| L4 | openclaw-memory-search | `0 */6 * * *` | Low | Memory search index health |

### Phase 7: Validation

```bash
# Health
docker exec openclaw-gateway node dist/index.js health --json

# Models auth
docker exec openclaw-gateway node dist/index.js models status

# Telegram
docker exec openclaw-gateway node dist/index.js channels status --probe

# Port binding (remote only)
ss -tlnp | grep 18789   # Must show 127.0.0.1:18789

# Doctor
docker exec openclaw-gateway node dist/index.js doctor

# Autopilot
stakpak autopilot status
stakpak autopilot schedule list
```

## Rollback Procedures

### Gateway Version Rollback

```bash
docker pull ghcr.io/openclaw/openclaw:<PREVIOUS_VERSION>
sed -i "s|openclaw:latest|openclaw:<PREVIOUS_VERSION>|" docker-compose.yml
docker compose up -d
```

### Telegram Token Rotation

```bash
docker exec openclaw-gateway node dist/index.js config set channels.telegram.botToken "<NEW_TOKEN>"
docker compose restart
```

## Known Gotchas

| # | Symptom | Cause | Fix |
|---|---------|-------|-----|
| 1 | `EACCES: permission denied` | Host dirs owned by wrong UID | `chown -R 1000:1000` before first start |
| 2 | LLM provider not found | Auth store not configured | Check docs and run `doctor` |
| 3 | `ANTHROPIC_API_KEY` env var ignored | OpenClaw reads from auth store, not env | Write auth-profiles.json per docs |
| 4 | Config key rejected | Key name changed between versions | Check configuration-reference docs |
| 5 | `gateway.mode is unset` | Not configured | `config set gateway.mode "local"` |
| 6 | `Connection reset by peer` on health | Fail-closed auth | Use CLI health check inside container |
| 7 | Old AMI doesn't exist | Hardcoded AMI ID | Query latest AL2023 AMI dynamically |
| 8 | `yum: command not found` | AL2023 uses dnf | Use `dnf`, not `yum` |
| 9 | Broken scripts via SSH heredoc | Shell escaping corruption | Write locally, SCP to host |
| 10 | Telegram 409 Conflict | Two processes polling same bot | Separate bots for OpenClaw and Stakpak |
| 11 | `Provider not found` in autopilot | No LLM provider configured | Run `stakpak auth login` |
| 12 | Discord `400 Bad Request` loop | Invalid Discord bot token | Regenerate at Discord Developer Portal |
| 13 | `database is locked` warnings | High-frequency schedules | Use `*/5` minimum; warnings are non-fatal |
| 14 | Schedule runs but no notification | Missing `--channel` flag | Always pass `--channel` |
| 15 | `missing gateway notifications config` | Channel added without `--target` | Re-add with `--target <CHAT_ID>` |
| 16 | IP changes after EC2 stop/start | No Elastic IP | Allocate EIP in Phase 1.5 |
| 17 | OOM kills container | No swap on 2GB instance | Add 2GB swap |
| 18 | SSH timeout after IP change | SG has old IP | Update SG ingress rule |
| 19 | Stale gateway lock file | SIGKILL/OOM left lock file | `rm -f /data/gateway.*.lock` |
| 20 | Heartbeat silently skipped | Queue saturated or delivery target missing | Check `logs --follow \| grep heartbeat` |
| 21 | Messages silently dropped | Queue overflow (cap: 20/session) | Increase `messages.queue.cap` or `maxConcurrent` |
| 22 | Context compaction burning tokens | Sessions accumulating large tool outputs | Enable `contextPruning.mode = "cache-ttl"` |
| 23 | Memory search silently disabled | Embedding API key expired or QMD binary missing | Check `status --all \| grep memory` |
| 24 | Gateway stops after SSH logout | systemd linger disabled | `loginctl enable-linger $USER` |
| 25 | Orphaned sandbox containers | Crash during sandboxed session | `docker rm $(docker ps -a --filter name=openclaw-sandbox --filter status=exited -q)` |
| 26 | Auth profile in exponential cooldown | Rate limits or billing failure | Wait for cooldown or top up provider account |
| 27 | Node runtime on Bun/version-manager | Wrong binary in service path | `doctor --repair` or reinstall with system Node |
| 28 | Sandbox image missing | Docker cleanup removed it | Re-run `scripts/sandbox-setup.sh` |

## Success Criteria

* [ ] Container running and healthy (`docker compose ps` shows `(healthy)`)
* [ ] `models status` shows provider `ok`
* [ ] `channels status --probe` shows Telegram running and `works`
* [ ] First user paired and can chat
* [ ] Port 18789 bound to 127.0.0.1 only
* [ ] `doctor` reports no critical issues
* [ ] Elastic IP assigned (EC2)
* [ ] fail2ban active, root SSH disabled, MaxAuthTries = 3
* [ ] 2GB swap configured and active
* [ ] EBS encrypted (EC2)
* [ ] systemd linger enabled (VPS)
* [ ] Node runtime is system Node, not Bun/nvm/fnm
* [ ] Stakpak autopilot running with all 17 schedules
* [ ] Autopilot notifications delivering (not silent mode)
* [ ] `schedule trigger --dry-run` shows checks passing
* [ ] All credentials stored with 600 permissions

## References

| Topic | URL |
|-------|-----|
| Docker deployment | https://docs.openclaw.ai/install/docker |
| Telegram channel | https://docs.openclaw.ai/channels/telegram |
| Configuration reference | https://docs.openclaw.ai/gateway/configuration-reference |
| Security hardening | https://docs.openclaw.ai/gateway/security/index |
| Health checks | https://docs.openclaw.ai/gateway/health |
| Model authentication | https://docs.openclaw.ai/gateway/authentication |
| Channel pairing | https://docs.openclaw.ai/channels/pairing |
| Stakpak CLI | https://github.com/stakpak/agent |
| Autopilot docs | https://stakpak.gitbook.io/docs |
| Telegram Bot API | https://core.telegram.org/bots/api |
| Discord Developer Portal | https://discord.com/developers/applications |
| Anthropic Console | https://console.anthropic.com/settings/keys |
| Caddy docs | https://caddyserver.com/docs/ |