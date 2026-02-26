
# Deploy & Monitor OpenClaw Gateway with Stakpak Autopilot

## Goals

* Deploy a fully functional OpenClaw AI gateway with Docker on any target (EC2, VPS, or local)
* Connect Telegram as the primary messaging channel with DM pairing security
* Harden the deployment for production (fail-closed auth, loopback binding, OS-level security)
* Configure Stakpak Autopilot for continuous health monitoring with Telegram or Discord alerts

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

> **Defer alert channel choice** to Phase 6. Only the deployment target is needed upfront.

### Required Tokens — Collect All Before Starting

| Token | Source | Format | Phase |
|-------|--------|--------|-------|
| Anthropic API key | https://console.anthropic.com/settings/keys | `sk-ant-api03-...` | 2 |
| **OpenClaw Telegram bot token** | Telegram → `@BotFather` → `/newbot` | `123456789:ABCdef...` | 3 |
| Telegram user ID | Telegram → `@userinfobot` → send any message | Numeric | 3 |
| **Stakpak alert channel token** | See Phase 6 — ask when configuring autopilot | Varies | 6 |
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

OpenClaw is a **single-user** multi-channel AI assistant gateway. It routes messages from 36+ messaging platforms to an AI agent. It is NOT multi-tenant.

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

**User data bootstrap**: Provision t3.small with Docker, fail2ban, 2GB swap, create `openclaw` user (UID 1000), `mkdir -p /opt/openclaw/{config,workspace,data,checks,logs}` owned by openclaw.

#### 1.2 Prepare Existing VPS

> For `$DEPLOY_TARGET` = "Existing VPS" — start here.

Write bootstrap script locally, SCP and execute (never SSH heredoc — see Core Principles). Same requirements as EC2: install Docker + fail2ban, 2GB swap, create `openclaw` user (UID 1000), `mkdir -p /opt/openclaw/{config,workspace,data,checks,logs}` owned by openclaw.

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

Generate 32-byte hex token with `openssl rand -hex 32`, store in `.env` as `OPENCLAW_GATEWAY_TOKEN=...`, `chmod 600` the `.env` file. Write script locally and SCP to remote; never SSH heredoc.

#### 2.3 Create Docker Compose File

> **Standalone production compose**: This differs from OpenClaw's repo `docker-setup.sh` — we create a minimal production compose with healthcheck, logging, and explicit state dir. Use this when you need a hardened deploy without the repo's onboarding flow.

```yaml
services:
  openclaw-gateway:
    image: ghcr.io/openclaw/openclaw:latest
    container_name: openclaw-gateway
    restart: unless-stopped
    command: ["node", "dist/index.js", "gateway", "--bind", "lan", "--port", "18789", "--allow-unconfigured"]
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
      - OPENCLAW_STATE_DIR=/data
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

**OpenClaw auth store format** — the LLM cannot infer this; it is proprietary. Write `auth-profiles.json` to `/home/node/.openclaw/agents/main/agent/auth-profiles.json` inside the container (or to the host path that maps to it). Example:

```json
{
  "version": 1,
  "profiles": {
    "anthropic:default": {
      "provider": "anthropic",
      "apiKey": "<ANTHROPIC_API_KEY>"
    }
  }
}
```

For other providers: `openai:default`, `gemini:default`, etc. Use `docker exec` to write or `docker cp` from host. Verify: `docker exec openclaw-gateway node dist/index.js models status` must show `ok`. Restart: `docker compose restart`.

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

Set up Caddy as reverse proxy to 127.0.0.1:18789 with websocket upgrade support and standard security headers.

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
```

#### 5.3 SSH Hardening (EC2/VPS only)

Standard SSH hardening: no root login, MaxAuthTries 3.

#### 5.4 AWS Hardening (EC2 only)

* Enforce IMDSv2: `aws ec2 modify-instance-metadata-options --instance-id $INSTANCE_ID --http-tokens required --http-endpoint enabled --region $REGION`
* Ensure EBS is encrypted (should already be if provisioned with `--encrypted` in Phase 1.1). For existing unencrypted volumes, use the standard snapshot → encrypted copy → volume swap procedure.

#### 5.5 Security Checklist (OpenClaw-specific)

* [ ] Gateway token set (32-byte hex, fail-closed)
* [ ] `auth-profiles.json` valid (`models status` shows `ok`)
* [ ] Port 18789 bound to 127.0.0.1 only
* [ ] File permissions: config dir = 700, openclaw.json = 600, .env = 600
* [ ] mDNS disabled, log redaction enabled
* [ ] Telegram DM policy = pairing, group policy = allowlist
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

Required for failure investigation. Without it: `Provider not found`.

```bash
stakpak auth list   # If empty:
stakpak auth login  # Select provider (Anthropic, OpenAI, DeepSeek, etc.), enter API key. Stored in ~/.stakpak/
```

> **Tip**: DeepSeek or Qwen work well for monitoring at lower cost.

##### Configure Alert Channel

Required for alerts. Without it, checks run silently.

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
stakpak autopilot channel list
```

If test fails:

| Symptom | Fix |
|---------|-----|
| `missing gateway notifications config` | You forgot `--target`. Re-add the channel with `--target <CHAT_ID>` |
| `401 Unauthorized` | Bot token is invalid or revoked. Regenerate at BotFather (Telegram) or Developer Portal (Discord) |
| `400 Bad Request` (Discord) | Bot token format is wrong, or bot wasn't invited to the server |
| `409 Conflict` (Telegram) | Another process is polling this bot token. Use a different bot |
| No message received | For Telegram: did you message the bot first? The chat must exist before Stakpak can send to it |

##### Start Autopilot Daemon

```bash
# Start autopilot (runs in background, survives terminal close):
stakpak up --non-interactive

# Check status:
stakpak autopilot status
```

Runs checks on cron; on failure: LLM investigates via SSH, sends results to alert channel. `--non-interactive` skips prompts.

> **If `stakpak up` fails**: `stakpak auth list` and `stakpak autopilot channel list` — both provider and channel must be configured.

#### 6.1 Check Scripts

**Order matters** — do Step 1 first, then Step 2. Wrappers call remote scripts; remote scripts must exist before wrappers work.

**Step 1**: Write check scripts locally (one per schedule in 6.3), SCP each to `/opt/openclaw/checks/<name>.sh` on the remote host. Never create via SSH heredoc.

**Step 2**: Create local SSH wrappers in `~/.stakpak/checks/` that invoke those remote scripts.

Each script should exit 0 on success, exit 1 on failure, and print a human-readable status line.

**Example — health.sh** (gateway process health):
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

Follow this pattern for all checks in the schedule table below. Each check script should:
* Use `docker exec openclaw-gateway node dist/index.js <command>` for OpenClaw CLI checks
* Use `docker inspect` for container status checks
* Use standard Linux tools (`df`, `free`, `du`) for resource checks
* Grep for failure keywords in output and exit 1 on match

**Step 2** (remote targets only): After Step 1 is done, create SSH wrapper scripts in `~/.stakpak/checks/`:
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

#### 6.2 Verify Alert Channel

> If you haven't configured the alert channel yet, go back to **Phase 6.0**.

```bash
stakpak autopilot channel test
stakpak autopilot channel list
```

#### 6.3 Add Schedules

> **ASK THE USER** at the start of Phase 6: "Which channel for Stakpak autopilot alerts — Telegram or Discord?" Store as `$ALERT_CHANNEL`. Use a **separate** Telegram bot from OpenClaw (409 Conflict if shared). See Phase 6.0 for channel setup.

**Two tiers** — Quick start first, then extend:
* **Quick start (4 checks)**: openclaw-health, openclaw-channels, openclaw-models, openclaw-resources — get alerts in ~30 min
* **Extended monitoring (remaining 13)**: add incrementally after the system is running

Every schedule MUST include `--channel $ALERT_CHANNEL`. Use `stakpak autopilot schedule add` for each row in the table below.

**Example:**
```bash
stakpak autopilot schedule add openclaw-health \
  --cron '*/5 * * * *' \
  --check ~/.stakpak/checks/openclaw-health.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 20 \
  --prompt "OpenClaw gateway health failed on $PUBLIC_IP. SSH in and investigate: docker logs openclaw-gateway --tail 100, docker ps. Restart if needed: cd /opt/openclaw && docker compose restart."
```

`--prompt` should describe the failure and suggest investigation steps:

| ID | Name | Cron | Steps | Check target | Prompt hint |
|----|------|------|-------|-------------|-------------|
| C1 | openclaw-health | `*/5 * * * *` | 20 | `health --json` | Gateway health, restart if needed |
| C2 | openclaw-service | `*/5 * * * *` | 15 | `docker inspect` container status + restart count | Container status, OOM, stale lock files |
| C3 | openclaw-channels | `*/5 * * * *` | 15 | `channels status --probe` | Channel disconnected/loggedOut, re-add token |
| C4 | openclaw-models | `*/15 * * * *` | 10 | `models status --check` (exit 1=expired, 2=expiring) | Model auth expired, update auth-profiles.json |
| H1 | openclaw-auth-cooldown | `*/30 * * * *` | 10 | grep auth-profiles.json for cooldownUntil/disabledReason | Auth cooldown or billing disabled |
| H2 | openclaw-resources | `0 */2 * * *` | 15 | disk >85%, memory <200MB, container unhealthy, Docker daemon | Disk/memory/container/Docker health |
| H3 | openclaw-sandbox-image | `0 */6 * * *` | 10 | `docker images openclaw-sandbox` exists | Sandbox image missing, run setup script |
| M1 | openclaw-cron | `*/15 * * * *` | 10 | `cron status` + `cron list` for errors | Cron scheduler disabled or job errors |
| M2 | openclaw-heartbeat | `*/30 * * * *` | 10 | `system heartbeat last` for skip reasons | Heartbeat delivery skipped |
| M3 | openclaw-queue | `*/10 * * * *` | 10 | grep recent logs for drop/overflow | Queue overflow, increase cap |
| M4 | openclaw-workspace | `0 */6 * * *` | 10 | `du -sm` workspace + agents >5GB | Workspace disk growth, archive old JSONL |
| M5 | openclaw-sandbox | `0 */1 * * *` | 10 | count exited openclaw-sandbox containers >10 | Orphaned sandbox containers |
| M6 | openclaw-compaction | `0 */1 * * *` | 10 | grep logs for compaction >10/day | Context compaction burning tokens |
| L1 | openclaw-security | `0 9 * * *` | 10 | file perms, `doctor`, DM policy, linger, runtime | Security audit, run doctor |
| L2 | openclaw-version | `0 9 * * 1` | 10 | compare running vs latest GitHub release | Version drift, do NOT auto-update |
| L3 | openclaw-presence | `0 */1 * * *` | 5 | `status --all` client count >10 | Unauthorized connections, rotate gateway token |
| L4 | openclaw-memory-search | `0 */6 * * *` | 10 | `status --all` grep memory errors | Memory search disabled, check embedding key |

#### 6.4 Start & Verify Autopilot

```bash
# If not already running from Phase 6.0:
stakpak up --non-interactive
stakpak autopilot status

# Dry-run a check to verify everything works end-to-end:
stakpak autopilot schedule trigger openclaw-health --dry-run
```

> Dry-run: runs check + LLM investigation if it fails, but does NOT send a notification.

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
| 7 | Broken scripts via SSH heredoc | Shell escaping corruption | Write locally, SCP to host |
| 8 | Telegram 409 Conflict | Two processes polling same bot | Separate bots for OpenClaw and Stakpak |
| 9 | `Provider not found` in autopilot | No LLM provider configured | Run `stakpak auth login` |
| 10 | Discord `400 Bad Request` loop | Invalid Discord bot token | Regenerate at Discord Developer Portal |
| 11 | `database is locked` warnings | High-frequency schedules | Use `*/5` minimum; warnings are non-fatal |
| 12 | Schedule runs but no notification | Missing `--channel` flag | Always pass `--channel` |
| 13 | `missing gateway notifications config` | Channel added without `--target` | Re-add with `--target <CHAT_ID>` |
| 14 | Stale gateway lock file | SIGKILL/OOM left lock file | `rm -f /data/gateway.*.lock` |
| 15 | Heartbeat silently skipped | Queue saturated or delivery target missing | Check `logs --follow \| grep heartbeat` |
| 16 | Messages silently dropped | Queue overflow (cap: 20/session) | Increase `messages.queue.cap` or `maxConcurrent` |
| 17 | Context compaction burning tokens | Sessions accumulating large tool outputs | Enable `contextPruning.mode = "cache-ttl"` |
| 18 | Memory search silently disabled | Embedding API key expired or QMD binary missing | Check `status --all \| grep memory` |
| 19 | Gateway stops after SSH logout | systemd linger disabled | `loginctl enable-linger $USER` |
| 20 | Orphaned sandbox containers | Crash during sandboxed session | `docker rm $(docker ps -a --filter name=openclaw-sandbox --filter status=exited -q)` |
| 21 | Auth profile in exponential cooldown | Rate limits or billing failure | Wait for cooldown or top up provider account |
| 22 | Node runtime on Bun/version-manager | Wrong binary in service path | `doctor --repair` or reinstall with system Node |
| 23 | Sandbox image missing | Docker cleanup removed it | Re-run `scripts/sandbox-setup.sh` |

## Success Criteria

* [ ] Container running and healthy (`docker compose ps` shows `(healthy)`)
* [ ] `models status` shows provider `ok`
* [ ] `channels status --probe` shows Telegram running and `works`
* [ ] First user paired and can chat
* [ ] Port 18789 bound to 127.0.0.1 only
* [ ] `doctor` reports no critical issues
* [ ] systemd linger enabled (VPS)
* [ ] Node runtime is system Node, not Bun/nvm/fnm
* [ ] Stakpak autopilot running (Quick start: 4 schedules minimum; extend to all 17 as needed)
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
| Autopilot docs | https://stakpak.gitbook.io/docs |
| Telegram Bot API | https://core.telegram.org/bots/api |
| Discord Developer Portal | https://discord.com/developers/applications |
| Anthropic Console | https://console.anthropic.com/settings/keys |
| Caddy docs | https://caddyserver.com/docs/ |