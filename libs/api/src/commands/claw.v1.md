---
description: Deploy & monitor OpenClaw gateway with Stakpak Autopilot
---

# Deploy & Monitor OpenClaw Gateway with AWS Lightsail Blueprint + Stakpak Autopilot

## Goals

* Deploy a fully functional OpenClaw AI gateway using the **AWS Lightsail OpenClaw blueprint** (one-click)
* Connect a messaging channel (Telegram, WhatsApp, Discord, or Slack) with security hardening
* Harden the deployment for production beyond Lightsail defaults
* Configure Stakpak Autopilot for continuous health monitoring with Telegram or Discord alerts

## Core Principles

* Collect ALL tokens and API keys before starting — every phase has blocking dependencies
* The Lightsail blueprint handles Docker, Node.js, systemd, HTTPS, and gateway token generation — do NOT manually install these
* Never share a Telegram bot token between OpenClaw and Stakpak — causes 409 Conflict polling errors (only applies if both use Telegram)
* The blueprint uses Amazon Bedrock (not `auth-profiles.json`) — only write `auth-profiles.json` if you want a non-Bedrock provider
* Use `openclaw` CLI directly via SSH (not `docker exec`) — the Lightsail blueprint runs OpenClaw as a native systemd service, not Docker
* Always verify with `openclaw channels status --probe` and `openclaw models status` after configuration changes
* Write scripts locally and SCP to remote hosts — never create scripts via SSH heredoc (shell escaping breaks)
* The blueprint locks SSH to browser-only (`lightsail-connect`) by default — you MUST open port 22 to your IP before any CLI-based SSH access (Phase 1.4)
* Channel plugins (telegram, discord, slack, whatsapp, etc.) are **disabled by default** — you MUST run `openclaw plugins enable <channel>` and restart the gateway before `openclaw channels add` will recognize the channel. Without this, `channels add --channel telegram` returns `Unknown channel: telegram`.
* The Bedrock IAM role name must match `LightsailRoleFor-<EC2-instance-id>` (get via instance metadata), and the trust policy must allow Lightsail's managed account (`002204026182`)

## Prerequisites

### Mandatory User Prompt: Deployment Target

> **ASK THE USER** before starting: "Where are you deploying OpenClaw?"
>
> | Target | Description | Phases to follow |
> |--------|-------------|-----------------|
> | **Lightsail Blueprint** (recommended) | AWS Lightsail with OpenClaw blueprint | Phase 1 → 2 → 3 → 4 → 5 → 6 |
> | **Existing VPS** | Any Linux server with SSH access (Hetzner, DigitalOcean, etc.) | Use the **Docker-based guide** instead — this guide is Lightsail-specific |
> | **Local Docker** | Developer machine | Use the **Docker-based guide** instead — this guide is Lightsail-specific |
>
> This guide covers the **Lightsail Blueprint** path only. For VPS/local Docker, refer to the Docker-based deployment guide.

> **Defer alert channel choice** to Phase 5. Only the deployment target and messaging channel are needed upfront.

### Mandatory User Prompt: Messaging Channel

> **ASK THE USER** (use `ask_user` tool) which messaging channel to connect to OpenClaw.
>
> **Question 1 — Channel choice** (`allow_custom: false`):
>
> | Option | Value | Description |
> |--------|-------|-------------|
> | Telegram | `telegram` | Requires a bot token from @BotFather and your Telegram user ID |
> | WhatsApp | `whatsapp` | Requires QR code pairing from your phone — no tokens needed upfront |
> | Discord | `discord` | Requires a Discord bot token from the Developer Portal |
> | Slack | `slack` | Requires a Slack bot token and app token |
> | Skip | `skip` | Configure messaging later — browser-only for now |
>
> Store the user's choice as `$MESSAGING_CHANNEL`.

> **Then, based on `$MESSAGING_CHANNEL`, ask for the required tokens** (use `ask_user` tool):
>
> **If Telegram** — ask two questions:
> 1. "Paste your Telegram bot token" (from @BotFather → `/newbot`, format: `123456789:ABCdef...`)
>    - Validate format: must match `^\d+:[A-Za-z0-9_-]+$` (numeric ID, colon, alphanumeric hash). Reject tokens starting with `xoxb-`, `xapp-`, `sk-`, or other non-Telegram prefixes — users commonly paste wrong platform tokens.
>    - Validate API: `curl -s "https://api.telegram.org/bot<TOKEN>/getMe" | jq .ok` must return `true`
> 2. "Paste your Telegram user ID" (from @userinfobot, format: numeric like `123456789`)
>    - Validate format: must be purely numeric. Reject if it contains letters or dashes.
>
> Store as `$OPENCLAW_BOT_TOKEN` and `$TELEGRAM_USER_ID`.
>
> **If Discord** — ask one question:
> 1. "Paste your Discord bot token" (from Developer Portal → Bot → Token)
>
> Store as `$DISCORD_BOT_TOKEN`.
>
> **If Slack** — ask two questions:
> 1. "Paste your Slack bot token" (format: `xoxb-...`)
> 2. "Paste your Slack app-level token" (format: `xapp-...`)
>
> Store as `$SLACK_BOT_TOKEN` and `$SLACK_APP_TOKEN`.
>
> **If WhatsApp** — no tokens needed upfront. QR code pairing happens interactively in Phase 3.
>
> **If Skip** — no tokens needed. Phase 3 is skipped entirely.

### Required Tokens Summary

| Token | Required when | Source | Format |
|-------|--------------|--------|--------|
| AWS account access | Always | AWS Console login | Console + CloudShell access |
| Telegram bot token | `$MESSAGING_CHANNEL` = telegram | @BotFather → `/newbot` | `123456789:ABCdef...` |
| Telegram user ID | `$MESSAGING_CHANNEL` = telegram | @userinfobot | Numeric |
| Discord bot token | `$MESSAGING_CHANNEL` = discord | Developer Portal → Bot | Token string |
| Slack bot token | `$MESSAGING_CHANNEL` = slack | Slack App settings | `xoxb-...` |
| Slack app token | `$MESSAGING_CHANNEL` = slack | Slack App settings | `xapp-...` |
| Stakpak alert token | Always (Phase 5) | See Phase 5 — asked later | Varies |

> **Note**: No Anthropic API key needed upfront — the blueprint uses Amazon Bedrock. You only need to complete the Anthropic First Time Use (FTU) form in the Bedrock console if using Anthropic models (the default is Claude Sonnet 4.6).

### Telegram Bot Setup (BotFather) — If Using Telegram

Only needed if `$MESSAGING_CHANNEL` = `telegram` or if Telegram is chosen for Stakpak alerts (Phase 5).

@BotFather → `/newbot` → token → `/setprivacy` Disable → validate `curl "https://api.telegram.org/bot<TOKEN>/getMe"` → **message the bot first** to create the chat.

> **⚠ If using Telegram for BOTH OpenClaw and Stakpak alerts**: Create TWO separate bots. Sharing a token causes 409 Conflict.

### Stakpak Autopilot LLM Provider

> **⚠ CRITICAL**: Before starting autopilot, verify that the Stakpak CLI has a working LLM provider.
> Run `stakpak auth list` to check. If empty, run `stakpak auth login` to configure one.
> Without this, every schedule will fail silently with `Provider not found`.

## OpenClaw on Lightsail — What the Blueprint Provides

### What It Is

The OpenClaw Lightsail blueprint (launched March 4, 2026) is a **pre-configured one-click deployment**. It is NOT a Docker deployment — OpenClaw runs as a native systemd service.

* **Blueprint**: Select "OpenClaw" under Linux/Unix in Lightsail console (blueprint ID: `openclaw_ls_1_0`, version `2026.2.17`)
* **Recommended plan**: 4 GB memory (`medium_3_0`, $24/month) — 2 vCPUs, 80 GB SSD, 4 TB transfer, public IPv4 included
* **IPv6-only option**: `medium_ipv6_3_0` at $20/month — saves $4 but requires IPv6 support from all clients
* **Default AI provider**: Amazon Bedrock with Anthropic Claude Sonnet 4.6
* **Service management**: `sudo systemctl {start|stop|status} openclaw-gateway`
* **Built-in HTTPS**: Let's Encrypt certificate auto-issued for the instance IP, auto-renewed every 7 days
* **Certificate daemon**: `lightsail-manage-certd` — handles IP changes (static IP attach/detach) automatically
* **Gateway token**: Auto-generated at first boot, shown in SSH welcome message
* **Sandboxing**: Built-in session isolation
* **Auto snapshots**: 7 daily snapshots when enabled

### What the Blueprint Handles (Skip These)

| Manual Step (Docker guide) | Blueprint Status |
|---|---|
| Provision VM, install Docker, swap, fail2ban | ✓ Handled |
| Docker compose, image pull, healthcheck | ✓ Native systemd service |
| Generate gateway token, `.env` file | ✓ Auto-generated at boot |
| Write `auth-profiles.json` for LLM | ✓ Bedrock pre-configured (CloudShell script for IAM) |
| TLS/Caddy reverse proxy | ✓ Built-in Let's Encrypt HTTPS |
| Volume permissions (UID 1000) | ✓ Pre-configured |
| `config set gateway.mode "local"` | ✓ Pre-configured |
| SSH hardening | ✓ Lightsail managed (browser-based SSH) |
| mDNS discovery | ✓ Not applicable |

### CLI Commands (Native — No `docker exec`)

On the Lightsail instance, use the `openclaw` CLI directly:

```bash
openclaw health --json              # Health check
openclaw status --all               # Full status
openclaw doctor                     # Diagnostics
openclaw channels status --probe    # Per-channel connectivity
openclaw channels add               # Interactive channel setup
openclaw models status              # LLM auth status
openclaw models status --check      # Machine-readable (0=ok, 1=expired, 2=expiring)
openclaw pairing approve telegram <CODE>  # Approve pairing
openclaw token rotate               # Rotate gateway token
openclaw cron status                # Cron scheduler status
openclaw cron list                  # List cron jobs
openclaw system heartbeat last      # Last heartbeat
openclaw logs --follow              # Stream live logs
openclaw sessions list              # Active sessions
openclaw security audit --deep      # Security audit
```

### Service Management

```bash
sudo systemctl stop openclaw-gateway
sudo systemctl start openclaw-gateway
sudo systemctl status openclaw-gateway
sudo systemctl restart openclaw-gateway
```

## Workflow

### Phase 1: Create Lightsail OpenClaw Instance

#### 1.1 Create Instance (Console)

1. Sign in to the [Lightsail console](https://lightsail.aws.amazon.com)
2. Choose **Create instance**
3. Select your preferred **AWS Region and Availability Zone**
4. Under **Select a platform** → **Linux/Unix**
5. Under **Select a blueprint** → **OpenClaw**
6. Under **Choose your instance plan** → **4 GB memory** ($24/month recommended)
7. Enter instance name (e.g., `openclaw-gateway`)
8. Choose **Create instance**
9. Wait for status to show **Running** (2–3 minutes)

#### 1.2 Create Instance (CLI Alternative)

```bash
# List available blueprints to find OpenClaw blueprint ID
aws lightsail get-blueprints --query 'blueprints[?contains(name, `OpenClaw`) || contains(name, `openclaw`)].{id:blueprintId,name:name,isActive:isActive}' --output table

# Create instance — blueprint ID is openclaw_ls_1_0, bundle medium_3_0 (4GB/$24mo)
# Note: Blueprint requires minPower 1000 — medium_3_0 and above qualify
aws lightsail create-instances \
  --instance-names openclaw-gateway \
  --availability-zone us-east-1a \
  --blueprint-id openclaw_ls_1_0 \
  --bundle-id medium_3_0

# Wait for instance to be running
aws lightsail get-instance-state --instance-name openclaw-gateway
```

#### 1.3 Attach Static IP (Recommended)

The default public IP changes on stop/start. Attach a static IP to keep it stable:

```bash
aws lightsail allocate-static-ip --static-ip-name openclaw-static-ip
aws lightsail attach-static-ip --static-ip-name openclaw-static-ip --instance-name openclaw-gateway

# Verify and store the IP for later use
export OPENCLAW_IP=$(aws lightsail get-static-ip --static-ip-name openclaw-static-ip --query 'staticIp.ipAddress' --output text)
echo "Static IP: $OPENCLAW_IP"
```

> The `lightsail-manage-certd` daemon will automatically detect the IP change and reissue the Let's Encrypt certificate.

#### 1.4 Open SSH Firewall for CLI Access

> **⚠ IMPORTANT**: The blueprint restricts SSH (port 22) to `lightsail-connect` only (browser-based SSH). If you want to SSH from your local machine (required for Phase 5 autopilot monitoring), you must open port 22 to your IP.

```bash
# Get your public IP
MY_IP=$(curl -s https://checkip.amazonaws.com)

# Open SSH to your IP only, keep HTTPS and HTTP open to all
aws lightsail put-instance-public-ports \
  --instance-name openclaw-gateway \
  --port-infos "[
    {\"fromPort\":22,\"toPort\":22,\"protocol\":\"tcp\",\"cidrs\":[\"${MY_IP}/32\"]},
    {\"fromPort\":443,\"toPort\":443,\"protocol\":\"tcp\",\"cidrs\":[\"0.0.0.0/0\"],\"ipv6Cidrs\":[\"::/0\"]},
    {\"fromPort\":80,\"toPort\":80,\"protocol\":\"tcp\",\"cidrs\":[\"0.0.0.0/0\"],\"ipv6Cidrs\":[\"::/0\"]}
  ]"
```

> **Note**: If you skip this step, you can still use browser-based SSH from the Lightsail console, but you won't be able to automate Phase 2/3 via CLI or set up autopilot SSH monitoring in Phase 5.

#### 1.5 Download SSH Key

```bash
# Download Lightsail default key pair — extract PEM from JSON (do NOT use base64 -d, the API returns raw PEM text)
aws lightsail download-default-key-pair --output json | python3 -c "
import sys, json, os
data = json.load(sys.stdin)
path = os.path.expanduser('~/.ssh/lightsail-openclaw.pem')
with open(path, 'w') as f:
    f.write(data['privateKeyBase64'])
print('Key saved to', path)
"
chmod 600 ~/.ssh/lightsail-openclaw.pem

# Test SSH connectivity (the default user is ubuntu)
ssh -i ~/.ssh/lightsail-openclaw.pem -o StrictHostKeyChecking=no ubuntu@$OPENCLAW_IP "openclaw health --json"
```

> **⚠ Key download gotcha**: The `privateKeyBase64` field name is misleading — the API returns the raw PEM text, NOT base64-encoded binary. Using `--output text | base64 -d` produces a corrupt key with `Load key: invalid format`. Always extract it via `python3` or `jq -r` from the JSON response.

### Phase 2: Browser Pairing & Bedrock Setup

#### 2.1 Pair Your Browser

1. In the Lightsail console, click your instance name → **Getting started** tab
2. Under **Pair your browser to OpenClaw**, click **Connect using SSH**
3. In the SSH terminal, locate the **Dashboard URL** and **Access Token** in the welcome message
4. Open the Dashboard URL in a new browser tab
5. Paste the Access Token into the **Gateway Token** field → click **Connect**
6. Return to SSH terminal → press `y` to continue → press `a` to approve
7. Dashboard should show **OK** status

#### 2.2 Enable Amazon Bedrock

**Option A: Console (CloudShell script)**

1. On the instance management page → **Getting started** tab
2. Under **Enable Amazon Bedrock as your model provider** → click **Copy the script**
3. Click **Launch CloudShell**
4. Paste the script into CloudShell → press **Enter**
5. Wait for **Done** in the output

**Option B: CLI (fully automated — preferred for automation)**

> **⚠ CRITICAL IAM details discovered during deployment:**
> - The role name MUST use the **EC2 instance ID** (format `i-xxxxxxxxx`), NOT the Lightsail UUID. Get it from instance metadata.
> - The trust policy MUST allow `arn:aws:iam::002204026182:role/AmazonLightsailInstance` — this is Lightsail's managed account that the instance uses to assume your role. The standard `lightsail.amazonaws.com` service principal alone is NOT sufficient.
> - The instance already has `AWS_PROFILE=assumed` configured in its systemd env file (`/opt/aws/open_claw/openclaw.env`), pointing to an AWS CLI profile in `~/.aws/config` that chains to your role via `credential_source = Ec2InstanceMetadata`. The blueprint sets this up at boot — you just need to create the IAM role it expects.

```bash
# Step 1: Get the EC2 instance ID (NOT the Lightsail UUID)
EC2_INSTANCE_ID=$(ssh -i ~/.ssh/lightsail-openclaw.pem -o StrictHostKeyChecking=no ubuntu@$OPENCLAW_IP \
  'TOKEN=$(curl -s -X PUT "http://169.254.169.254/latest/api/token" -H "X-aws-ec2-metadata-token-ttl-seconds: 21600"); curl -s -H "X-aws-ec2-metadata-token: $TOKEN" http://169.254.169.254/latest/meta-data/instance-id')
echo "EC2 Instance ID: $EC2_INSTANCE_ID"

# Step 2: Get your AWS account ID
ACCOUNT_ID=$(aws sts get-caller-identity --query 'Account' --output text)

# Step 3: Create the IAM role with the correct trust policy
ROLE_NAME="LightsailRoleFor-${EC2_INSTANCE_ID}"

cat > /tmp/lightsail-trust-policy.json << EOF
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::002204026182:role/AmazonLightsailInstance"
      },
      "Action": "sts:AssumeRole"
    },
    {
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::002204026182:root"
      },
      "Action": "sts:AssumeRole"
    },
    {
      "Effect": "Allow",
      "Principal": {
        "Service": "lightsail.amazonaws.com"
      },
      "Action": "sts:AssumeRole"
    }
  ]
}
EOF

aws iam create-role \
  --role-name "$ROLE_NAME" \
  --assume-role-policy-document file:///tmp/lightsail-trust-policy.json \
  --description "Bedrock access for Lightsail OpenClaw instance"

# Step 4: Attach Bedrock + Marketplace permissions
cat > /tmp/lightsail-bedrock-policy.json << EOF
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "bedrock:InvokeModel",
        "bedrock:InvokeModelWithResponseStream",
        "bedrock:ListFoundationModels",
        "bedrock:GetFoundationModel"
      ],
      "Resource": "*"
    },
    {
      "Effect": "Allow",
      "Action": [
        "aws-marketplace:Subscribe",
        "aws-marketplace:Unsubscribe",
        "aws-marketplace:ViewSubscriptions"
      ],
      "Resource": "*"
    }
  ]
}
EOF

aws iam put-role-policy \
  --role-name "$ROLE_NAME" \
  --policy-name "BedrockAndMarketplaceAccess" \
  --policy-document file:///tmp/lightsail-bedrock-policy.json

echo "✓ IAM role created: $ROLE_NAME"

# Step 5: Verify from the instance (wait ~10s for IAM propagation)
sleep 10
ssh -i ~/.ssh/lightsail-openclaw.pem -o StrictHostKeyChecking=no ubuntu@$OPENCLAW_IP \
  'AWS_PROFILE=assumed aws bedrock list-foundation-models --region us-east-1 --query "modelSummaries[0].modelId" --output text'
```

> **What this does**: Creates an IAM role (`LightsailRoleFor-<ec2-instance-id>`) with Bedrock API access and AWS Marketplace permissions. The instance's pre-configured `AWS_PROFILE=assumed` in `~/.aws/config` chains to this role automatically via `credential_source = Ec2InstanceMetadata`.

#### 2.3 Anthropic First Time Use (One-Time)

If this is your first time using Anthropic models in your AWS account:

1. Open the [Amazon Bedrock console](https://console.aws.amazon.com/bedrock/)
2. Navigate to **Model catalog** → select an Anthropic model (Claude)
3. Complete the **First Time Use** form — access is granted immediately

> **Note**: Models from Amazon, Meta, Mistral AI, DeepSeek, and Qwen do NOT require this step.

#### 2.4 Verify

Navigate to **Chat** in the OpenClaw dashboard and send a test message. If it responds, Bedrock is working.

SSH into the instance and verify:
```bash
openclaw health --json
openclaw models status
```

### Phase 3: Messaging Channel Integration

> **Skip this phase** if `$MESSAGING_CHANNEL` = `skip`.

#### 3.1 Enable Channel Plugin & Add Channel

SSH into the instance and enable the channel plugin first, then configure using the tokens collected in Prerequisites.

> **⚠ IMPORTANT**: Channel plugins are **disabled by default** on the Lightsail blueprint. You must enable the plugin and restart the gateway before `channels add` will work. The `--channel` value is **lowercase** — use `telegram`, `whatsapp`, `discord`, `slack`.

**If `$MESSAGING_CHANNEL` = `telegram`:**

```bash
# Enable the telegram plugin (disabled by default)
openclaw plugins enable telegram
sudo systemctl restart openclaw-gateway
sleep 10

# Add the channel non-interactively (lowercase "telegram")
openclaw channels add --channel telegram --token "$OPENCLAW_BOT_TOKEN"
```

Then configure security and pre-approve the user:

```bash
openclaw config set channels.telegram.dmPolicy "pairing"
openclaw config set channels.telegram.groupPolicy "allowlist"
openclaw config set channels.telegram.allowFrom "[$TELEGRAM_USER_ID]"
openclaw config set channels.telegram.mediaMaxMb 5
openclaw config set channels.telegram.actions.sendMessage true
openclaw config set channels.telegram.actions.reactions true
openclaw config set channels.telegram.reactionLevel "minimal"
```

**If `$MESSAGING_CHANNEL` = `whatsapp`:**

```bash
# Enable the whatsapp plugin (disabled by default)
openclaw plugins enable whatsapp
sudo systemctl restart openclaw-gateway
sleep 10

# WhatsApp requires interactive QR code pairing — cannot be fully automated
openclaw channels add --channel whatsapp
# A QR code will be displayed in the terminal
# On your phone: WhatsApp → Settings → Linked Devices → scan the QR code
# Complete pairing on your phone
```

> No tokens needed — WhatsApp uses QR code pairing directly.

**If `$MESSAGING_CHANNEL` = `discord`:**

```bash
# Enable the discord plugin (disabled by default)
openclaw plugins enable discord
sudo systemctl restart openclaw-gateway
sleep 10

openclaw channels add --channel discord --token "$DISCORD_BOT_TOKEN"
```

**If `$MESSAGING_CHANNEL` = `slack`:**

```bash
# Enable the slack plugin (disabled by default)
openclaw plugins enable slack
sudo systemctl restart openclaw-gateway
sleep 10

openclaw channels add --channel slack --bot-token "$SLACK_BOT_TOKEN" --app-token "$SLACK_APP_TOKEN"
```

#### 3.2 Restart & Verify

```bash
sudo systemctl restart openclaw-gateway
# Wait 15 seconds
openclaw channels status --probe
# Must show: $MESSAGING_CHANNEL: enabled, configured, running, works
```

#### 3.3 Approve First User (Telegram only)

> **Skip** if `$MESSAGING_CHANNEL` ≠ `telegram`. WhatsApp/Discord/Slack handle auth differently.

If you pre-approved via `allowFrom` in 3.1, send a test message to the bot — it should respond.

If using pairing mode instead, send a message to the bot in Telegram, then approve in SSH:

```bash
openclaw pairing approve telegram <PAIRING_CODE>
```

> **Discovery note**: Config keys evolve between versions. If a key is rejected, check https://docs.openclaw.ai/gateway/configuration-reference or run `openclaw doctor` for diagnostics.

### Phase 4: Security Hardening

The Lightsail blueprint provides a solid baseline. These steps add defense-in-depth.

#### 4.1 Rotate Gateway Token (If Exposed)

The token is shown in the SSH welcome message. If anyone else accessed it:

```bash
openclaw token rotate
# Re-pair all browsers with the new token
```

#### 4.2 OpenClaw Application Security

```bash
# Create log directory (does not exist by default)
sudo mkdir -p /data/logs
sudo chown ubuntu:ubuntu /data/logs

# Fix missing directories that cause CRITICAL doctor warnings
mkdir -p ~/.openclaw/agents/main/sessions
mkdir -p ~/.openclaw/credentials
chmod 700 ~/.openclaw/credentials

openclaw config set logging.redactSensitive "tools"
openclaw config set logging.file "/data/logs/openclaw.log"
```

#### 4.3 Firewall Rules

In the Lightsail console or via CLI, verify firewall rules:

```bash
# List current firewall rules
aws lightsail get-instance-port-states --instance-name openclaw-gateway

# Ensure only SSH (22) and HTTPS (443) are open
# The blueprint should NOT expose port 18789 publicly
# If it does, close it:
aws lightsail close-instance-public-ports \
  --instance-name openclaw-gateway \
  --port-info fromPort=18789,toPort=18789,protocol=tcp
```

> **⚠ CRITICAL**: Never expose port 18789 to the public internet. The built-in HTTPS endpoint handles browser access. Telegram uses outbound polling — no inbound ports needed.

#### 4.4 Restrict SSH Access

> **Note**: If you completed Phase 1.4, SSH is already restricted to your IP. If your IP changes, update the firewall:

```bash
MY_IP=$(curl -s https://checkip.amazonaws.com)
aws lightsail put-instance-public-ports \
  --instance-name openclaw-gateway \
  --port-infos "[
    {\"fromPort\":22,\"toPort\":22,\"protocol\":\"tcp\",\"cidrs\":[\"${MY_IP}/32\"]},
    {\"fromPort\":443,\"toPort\":443,\"protocol\":\"tcp\",\"cidrs\":[\"0.0.0.0/0\"]},
    {\"fromPort\":80,\"toPort\":80,\"protocol\":\"tcp\",\"cidrs\":[\"0.0.0.0/0\"]}
  ]"
```

#### 4.5 Enable Automatic Snapshots

```bash
aws lightsail enable-add-on \
  --resource-name openclaw-gateway \
  --add-on-request addOnType=AutoSnapshot,autoSnapshotAddOnRequest={snapshotTimeOfDay=06:00}
```

#### 4.6 Security Checklist (OpenClaw-specific)

* [ ] Gateway token rotated from initial boot value (if welcome message was shared)
* [ ] `models status` shows provider `ok`
* [ ] Port 18789 NOT exposed in Lightsail firewall
* [ ] HTTPS working (Let's Encrypt cert active)
* [ ] Telegram DM policy = pairing, group policy = allowlist (if using Telegram)
* [ ] Log redaction enabled
* [ ] Automatic snapshots enabled
* [ ] `openclaw doctor` reports no critical issues
* [ ] `openclaw security audit --deep` reviewed

### Phase 5: Stakpak Autopilot Monitoring

Stakpak Autopilot runs on your **LOCAL machine** and monitors the Lightsail instance via SSH.

#### 5.0 Install & Configure Stakpak CLI

##### Install Stakpak

```bash
# macOS (Homebrew):
brew tap stakpak/stakpak && brew install stakpak

# Linux / macOS (curl):
curl -sSL https://stakpak.dev/install.sh | sh

# Verify:
stakpak version
```

##### Configure LLM Provider

Required for failure investigation. Without it: `Provider not found`.

```bash
stakpak auth list   # If empty:
stakpak auth login  # Select provider (Anthropic, OpenAI, DeepSeek, etc.), enter API key
```

> **Tip**: DeepSeek or Qwen work well for monitoring at lower cost.

##### Configure Alert Channel

Required for alerts. Without it, checks run silently.

> **ASK THE USER** (use `ask_user` tool) which channel for Stakpak autopilot alerts.
>
> **Question — Alert channel choice** (`allow_custom: false`):
>
> | Option | Value | Description |
> |--------|-------|-------------|
> | Telegram | `telegram` | Requires a **separate** bot from OpenClaw (if OpenClaw also uses Telegram) |
> | Discord | `discord` | Requires a Discord bot token |
> | Slack | `slack` | Requires Slack bot + app tokens |
>
> Store the user's choice as `$ALERT_CHANNEL`.

> **Then, based on `$ALERT_CHANNEL`, ask for the required tokens** (use `ask_user` tool):
>
> **If Telegram** — ask two questions:
> 1. "Paste your Stakpak alert Telegram bot token" (must be a DIFFERENT bot than `$OPENCLAW_BOT_TOKEN` if OpenClaw uses Telegram)
>    - Validate: `curl -s "https://api.telegram.org/bot<TOKEN>/getMe" | jq .ok` must return `true`
>    - Remind user: "Send the bot a message first (e.g. 'hi') to create the chat"
> 2. "Paste your Telegram chat ID for alerts" (get via `curl -s "https://api.telegram.org/bot<TOKEN>/getUpdates" | jq '.result[0].message.chat.id'`)
>
> Store as `$STAKPAK_BOT_TOKEN` and `$STAKPAK_CHAT_ID`.
>
> **If Discord** — ask one question:
> 1. "Paste your Discord bot token for Stakpak alerts"
>
> Store as `$STAKPAK_DISCORD_TOKEN`.
>
> **If Slack** — ask two questions:
> 1. "Paste your Slack bot token for Stakpak alerts" (format: `xoxb-...`)
> 2. "Paste your Slack app-level token" (format: `xapp-...`)
>
> Store as `$STAKPAK_SLACK_BOT_TOKEN` and `$STAKPAK_SLACK_APP_TOKEN`.

Then configure the channel:

**If `$ALERT_CHANNEL` = `telegram`:**

```bash
stakpak autopilot channel add telegram --token $STAKPAK_BOT_TOKEN --target $STAKPAK_CHAT_ID
```

> **⚠ CRITICAL**: The `--target $STAKPAK_CHAT_ID` flag is REQUIRED. Without it, autopilot runs silently (`missing gateway notifications config`). If you forgot `--target`, remove and re-add:
> ```bash
> stakpak autopilot channel remove telegram
> stakpak autopilot channel add telegram --token $STAKPAK_BOT_TOKEN --target $STAKPAK_CHAT_ID
> ```

**If `$ALERT_CHANNEL` = `discord`:**

```bash
stakpak autopilot channel add discord --token $STAKPAK_DISCORD_TOKEN
```

**If `$ALERT_CHANNEL` = `slack`:**

```bash
stakpak autopilot channel add slack --bot-token $STAKPAK_SLACK_BOT_TOKEN --app-token $STAKPAK_SLACK_APP_TOKEN
```

##### Verify Channel

```bash
stakpak autopilot channel test
stakpak autopilot channel list
```

If test fails:

| Symptom | Fix |
|---------|-----|
| `missing gateway notifications config` | You forgot `--target`. Re-add with `--target <CHAT_ID>` |
| `401 Unauthorized` | Bot token invalid or revoked |
| `409 Conflict` (Telegram) | Another process polling same bot. Use a different bot |
| No message received | Did you message the bot first? Chat must exist |

##### Start Autopilot Daemon

```bash
stakpak up --non-interactive
stakpak autopilot status
```

#### 5.1 SSH Key Setup for Monitoring

Autopilot needs SSH access to the Lightsail instance. If you completed Phase 1.5 (Download SSH Key), you already have this.

```bash
# If you haven't set these yet:
export OPENCLAW_IP=$(aws lightsail get-static-ip --static-ip-name openclaw-static-ip --query 'staticIp.ipAddress' --output text)

# Verify SSH works
ssh -i ~/.ssh/lightsail-openclaw.pem -o StrictHostKeyChecking=no ubuntu@$OPENCLAW_IP "openclaw health --json"
```

> **Note**: The default SSH user for Lightsail OpenClaw instances is `ubuntu`. You can also verify by running: `aws lightsail get-instance-access-details --instance-name openclaw-gateway --protocol ssh --query 'accessDetails.username' --output text`

#### 5.2 Check Scripts

**Step 1**: Write check scripts locally, SCP each to `/opt/openclaw/checks/<name>.sh` on the Lightsail instance. Never create via SSH heredoc.

Each script should:
* Use `openclaw <command>` directly (NOT `docker exec`)
* Use `sudo systemctl status openclaw-gateway` for service checks
* Use standard Linux tools (`df`, `free`, `du`) for resource checks
* Exit 0 on success, exit 1 on failure
* Print a human-readable status line

**Example — health.sh**:
```bash
#!/bin/bash
OUTPUT=$(openclaw health --json 2>&1)
EXIT_CODE=$?
if [ $EXIT_CODE -ne 0 ]; then
  echo "FAIL: OpenClaw health check failed"
  echo "$OUTPUT"
  exit 1
fi
echo "OK: OpenClaw healthy"
echo "$OUTPUT"
exit 0
```

**Example — channels.sh**:
```bash
#!/bin/bash
OUTPUT=$(openclaw channels status --probe 2>&1)
if echo "$OUTPUT" | grep -qiE "disconnected|loggedOut|error|failed"; then
  echo "FAIL: Channel issue detected"
  echo "$OUTPUT"
  exit 1
fi
echo "OK: All channels healthy"
echo "$OUTPUT"
exit 0
```

**Example — models.sh**:
```bash
#!/bin/bash
openclaw models status --check
EXIT_CODE=$?
if [ $EXIT_CODE -eq 1 ]; then
  echo "FAIL: Model auth expired"
  exit 1
elif [ $EXIT_CODE -eq 2 ]; then
  echo "WARN: Model auth expiring soon"
  exit 1
fi
echo "OK: Models authenticated"
exit 0
```

**Example — resources.sh**:
```bash
#!/bin/bash
FAIL=0

# Disk check (>85%)
DISK_PCT=$(df / | awk 'NR==2 {gsub(/%/,""); print $5}')
if [ "$DISK_PCT" -gt 85 ]; then
  echo "FAIL: Disk usage at ${DISK_PCT}%"
  FAIL=1
fi

# Memory check (<200MB free)
FREE_MB=$(free -m | awk '/^Mem:/ {print $7}')
if [ "$FREE_MB" -lt 200 ]; then
  echo "FAIL: Only ${FREE_MB}MB memory available"
  FAIL=1
fi

# Service check
if ! sudo systemctl is-active --quiet openclaw-gateway; then
  echo "FAIL: openclaw-gateway service not running"
  FAIL=1
fi

if [ $FAIL -eq 1 ]; then exit 1; fi
echo "OK: Resources healthy (disk: ${DISK_PCT}%, free mem: ${FREE_MB}MB)"
exit 0
```

**Deploy check scripts to instance**:
```bash
# Create checks directory on instance
ssh -i ~/.ssh/lightsail-openclaw.pem ubuntu@$OPENCLAW_IP "sudo mkdir -p /opt/openclaw/checks && sudo chown ubuntu:ubuntu /opt/openclaw/checks"

# SCP all check scripts
scp -i ~/.ssh/lightsail-openclaw.pem checks/*.sh ubuntu@$OPENCLAW_IP:/opt/openclaw/checks/

# Make executable
ssh -i ~/.ssh/lightsail-openclaw.pem ubuntu@$OPENCLAW_IP "chmod +x /opt/openclaw/checks/*.sh"
```

**Step 2**: Create local SSH wrappers in `~/.stakpak/checks/`:

```bash
mkdir -p ~/.stakpak/checks

SSH_KEY=~/.ssh/lightsail-openclaw.pem
SSH_USER=ubuntu
# Use static IP if attached, otherwise get current public IP
PUBLIC_IP=$OPENCLAW_IP

for NAME in health service channels models auth-cooldown resources cron-status heartbeat queue workspace-disk orphaned-sandbox compaction presence memory-search sandbox-image security-audit version; do
  cat > ~/.stakpak/checks/openclaw-${NAME}.sh << EOF
#!/bin/bash
ssh -i $SSH_KEY -o StrictHostKeyChecking=no -o ConnectTimeout=10 \
  $SSH_USER@$PUBLIC_IP '/opt/openclaw/checks/${NAME}.sh'
EOF
  chmod +x ~/.stakpak/checks/openclaw-${NAME}.sh
done
```

#### 5.3 Verify Alert Channel

```bash
stakpak autopilot channel test
stakpak autopilot channel list
```

#### 5.4 Add Schedules

**Two tiers** — Quick start first, then extend:
* **Quick start (4 checks)**: openclaw-health, openclaw-channels, openclaw-models, openclaw-resources
* **Extended monitoring (remaining 13)**: add incrementally after the system is running

Every schedule MUST include `--channel $ALERT_CHANNEL`.

**Example:**
```bash
stakpak autopilot schedule add openclaw-health \
  --cron '*/5 * * * *' \
  --check ~/.stakpak/checks/openclaw-health.sh \
  --trigger-on failure --channel $ALERT_CHANNEL --max-steps 20 \
  --prompt "OpenClaw gateway health failed on $PUBLIC_IP. SSH in and investigate: openclaw health --json, openclaw doctor, sudo systemctl status openclaw-gateway, sudo journalctl -u openclaw-gateway --no-pager -n 100. Restart if needed: sudo systemctl restart openclaw-gateway."
```

**Full schedule table:**

| ID | Name | Cron | Steps | Check target | Prompt hint |
|----|------|------|-------|-------------|-------------|
| C1 | openclaw-health | `*/5 * * * *` | 20 | `openclaw health --json` | Gateway health, restart `systemctl restart openclaw-gateway` |
| C2 | openclaw-service | `*/5 * * * *` | 15 | `systemctl status openclaw-gateway` | Service status, OOM, stale lock files |
| C3 | openclaw-channels | `*/5 * * * *` | 15 | `openclaw channels status --probe` | Channel disconnected, re-add with `openclaw channels add` |
| C4 | openclaw-models | `*/15 * * * *` | 10 | `openclaw models status --check` | Model auth expired, check Bedrock IAM role |
| H1 | openclaw-auth-cooldown | `*/30 * * * *` | 10 | grep auth config for cooldown/disabled | Auth cooldown or billing disabled |
| H2 | openclaw-resources | `0 */2 * * *` | 15 | disk >85%, memory <200MB, service health | Disk/memory/service health |
| H3 | openclaw-sandbox-image | `0 */6 * * *` | 10 | sandbox image exists | Sandbox image missing |
| M1 | openclaw-cron | `*/15 * * * *` | 10 | `openclaw cron status` + `cron list` | Cron scheduler disabled or job errors |
| M2 | openclaw-heartbeat | `*/30 * * * *` | 10 | `openclaw system heartbeat last` | Heartbeat delivery skipped |
| M3 | openclaw-queue | `*/10 * * * *` | 10 | grep recent logs for drop/overflow | Queue overflow |
| M4 | openclaw-workspace | `0 */6 * * *` | 10 | `du -sm` workspace >5GB | Workspace disk growth |
| M5 | openclaw-sandbox | `0 */1 * * *` | 10 | count orphaned sandbox containers | Orphaned sandbox containers |
| M6 | openclaw-compaction | `0 */1 * * *` | 10 | grep logs for compaction >10/day | Context compaction burning tokens |
| L1 | openclaw-security | `0 9 * * *` | 10 | `openclaw doctor`, `openclaw security audit --deep` | Security audit |
| L2 | openclaw-version | `0 9 * * 1` | 10 | compare running vs latest release | Version drift — do NOT auto-update |
| L3 | openclaw-presence | `0 */1 * * *` | 5 | `openclaw status --all` client count >10 | Unauthorized connections, rotate token |
| L4 | openclaw-memory-search | `0 */6 * * *` | 10 | `openclaw status --all` grep memory errors | Memory search disabled |

#### 5.5 Start & Verify Autopilot

```bash
# If not already running:
stakpak up --non-interactive
stakpak autopilot status

# Dry-run a check:
stakpak autopilot schedule trigger openclaw-health --dry-run
```

### Phase 6: Validation

```bash
# SSH into instance
ssh -i ~/.ssh/lightsail-openclaw.pem ubuntu@$OPENCLAW_IP

# Health
openclaw health --json

# Models auth
openclaw models status

# Messaging channel (if configured)
openclaw channels status --probe

# Doctor
openclaw doctor

# Service status
sudo systemctl status openclaw-gateway

# HTTPS cert (from local machine)
curl -sI https://$OPENCLAW_IP | head -5

# Firewall (from local machine)
aws lightsail get-instance-port-states --instance-name openclaw-gateway

# Autopilot (on local machine)
stakpak autopilot status
stakpak autopilot schedule list
```

## Rollback Procedures

### Instance Restore from Snapshot

```bash
# List snapshots
aws lightsail get-instance-snapshots --query 'instanceSnapshots[].{name:name,createdAt:createdAt,state:state}' --output table

# Create new instance from snapshot
aws lightsail create-instances-from-snapshot \
  --instance-names openclaw-gateway-restored \
  --instance-snapshot-name <SNAPSHOT_NAME> \
  --availability-zone us-east-1a \
  --bundle-id medium_3_0

# Re-attach static IP to new instance
aws lightsail detach-static-ip --static-ip-name openclaw-static-ip
aws lightsail attach-static-ip --static-ip-name openclaw-static-ip --instance-name openclaw-gateway-restored
```

### Gateway Token Rotation

```bash
ssh -i ~/.ssh/lightsail-openclaw.pem ubuntu@$OPENCLAW_IP "openclaw token rotate"
# Re-pair all browsers
```

### Messaging Channel Credential Rotation

Credentials are stored in `~/.openclaw/credentials/` on the instance. SSH in and run `openclaw channels update`, then select the channel:

```bash
ssh -i ~/.ssh/lightsail-openclaw.pem ubuntu@$OPENCLAW_IP "openclaw channels update"
# Select the channel to update, enter new credentials when prompted
```

Per-channel steps to get new credentials:
* **Telegram**: Message `@BotFather` → `/revoke` → get new token
* **WhatsApp**: Log out from phone (`Settings → Linked Devices → Log out`) → re-pair with QR code
* **Discord**: Regenerate token at Developer Portal → Bot → Reset Token
* **Slack**: Regenerate tokens in Slack app settings

## Known Gotchas

| # | Symptom | Cause | Fix |
|---|---------|-------|-----|
| 1 | Chat not responding | Bedrock IAM role not set up | Run CloudShell script from Getting Started tab, or create IAM role via CLI (see Phase 2.2 Option B) |
| 2 | "First Time Use" error for Anthropic | FTU form not completed | Complete at Bedrock console → Model catalog → Anthropic |
| 3 | Dashboard unreachable after stop/start | IP changed (no static IP) | Attach a static IP via console/CLI |
| 4 | HTTPS cert error after IP change | `lightsail-manage-certd` reissuing | Wait 2–3 minutes for automatic reissue |
| 5 | Config key rejected | Key name changed between versions | Check https://docs.openclaw.ai/gateway/configuration-reference or run `openclaw doctor` |
| 6 | Telegram 409 Conflict | Two processes polling same bot | Separate bots for OpenClaw and Stakpak (only if both use Telegram) |
| 7 | `Provider not found` in autopilot | No LLM provider configured | Run `stakpak auth login` |
| 8 | Schedule runs but no notification | Missing `--channel` flag | Always pass `--channel` |
| 9 | `missing gateway notifications config` | Channel added without `--target` | Re-add with `--target <CHAT_ID>` |
| 10 | `database is locked` warnings | High-frequency schedules | Use `*/5` minimum; warnings are non-fatal |
| 11 | Broken scripts via SSH heredoc | Shell escaping corruption | Write locally, SCP to host |
| 12 | Heartbeat silently skipped | Queue saturated or delivery target missing | Check `openclaw logs --follow \| grep heartbeat` |
| 13 | Messages silently dropped | Queue overflow (cap: 20/session) | Increase `messages.queue.cap` or `maxConcurrent` |
| 14 | Context compaction burning tokens | Large tool outputs | Enable `contextPruning.mode = "cache-ttl"` |
| 15 | Memory search disabled | Embedding API key expired or QMD binary missing | Check `openclaw status --all \| grep memory` |
| 16 | Orphaned sandbox containers | Crash during sandbox session | Clean up exited containers |
| 17 | Auth profile in cooldown | Rate limits or billing failure | Wait for cooldown or check Bedrock quotas |
| 18 | Gateway token compromised | Exposed in logs or prompt injection | `openclaw token rotate` immediately |
| 19 | Bedrock permissions broken | IAM role modified | Re-run CloudShell script or restore IAM policy |
| 20 | Plugin requires restart | Config change not picked up | `sudo systemctl restart openclaw-gateway` |
| 21 | SSH times out to Lightsail instance | Blueprint restricts port 22 to `lightsail-connect` (browser SSH) only | Open port 22 to your IP: `aws lightsail put-instance-public-ports` (see Phase 1.4) |
| 22 | `Load key: invalid format` when SSH-ing | SSH key downloaded via `--output text \| base64 -d` | The `privateKeyBase64` field is raw PEM, not base64. Use `python3`/`jq -r` to extract from JSON (see Phase 1.5) |
| 23 | `Unknown channel: telegram` | Channel plugin is disabled by default | Run `openclaw plugins enable telegram` (or discord/slack/whatsapp) then `sudo systemctl restart openclaw-gateway` before running `channels add` |
| 24 | `AccessDenied` when instance assumes IAM role | Trust policy only has `lightsail.amazonaws.com` principal | Must also allow `arn:aws:iam::002204026182:role/AmazonLightsailInstance` (see Phase 2.2 Option B) |
| 25 | IAM role name wrong, Bedrock still fails | Used Lightsail UUID instead of EC2 instance ID | Role name must be `LightsailRoleFor-i-xxxxxxxxx` (EC2 ID from instance metadata), not the Lightsail UUID |
| 26 | `openclaw doctor` shows CRITICAL: session/credentials dirs missing | Blueprint doesn't create these dirs | `mkdir -p ~/.openclaw/agents/main/sessions ~/.openclaw/credentials && chmod 700 ~/.openclaw/credentials` (see Phase 4.2) |
| 27 | Logging config set but no log file written | `/data/logs/` directory doesn't exist | `sudo mkdir -p /data/logs && sudo chown ubuntu:ubuntu /data/logs` before setting `logging.file` |

## Success Criteria

* [ ] Lightsail instance running (status: Running)
* [ ] Browser paired with OpenClaw dashboard (status: OK)
* [ ] Bedrock IAM role created (CloudShell script completed)
* [ ] Chat working (test message gets AI response)
* [ ] `openclaw models status` shows provider `ok`
* [ ] `openclaw channels status --probe` shows `$MESSAGING_CHANNEL` running and `works` (if configured)
* [ ] First user paired and can chat via chosen channel (if configured)
* [ ] Port 18789 NOT exposed in Lightsail firewall
* [ ] HTTPS working (Let's Encrypt cert valid)
* [ ] Static IP attached (IP stable across stop/start)
* [ ] Automatic snapshots enabled
* [ ] `openclaw doctor` reports no critical issues
* [ ] Gateway token rotated from initial value (if shared)
* [ ] Stakpak autopilot running (Quick start: 4 schedules minimum)
* [ ] Autopilot notifications delivering (not silent mode)
* [ ] `schedule trigger --dry-run` shows checks passing

## Cost Breakdown

| Component | Monthly Cost | Notes |
|-----------|-------------|-------|
| Lightsail 4 GB plan (`medium_3_0`) | $24 | 2 vCPUs, 80 GB SSD, 4 TB transfer, IPv4 |
| Lightsail 4 GB IPv6-only (`medium_ipv6_3_0`) | $20 | Same specs, no IPv4 — saves $4/mo |
| Bedrock tokens (Claude Sonnet 4.6) | ~$1–10 | Varies with usage (few dozen messages/day = single digits) |
| Anthropic Marketplace fee | Included in token price | One-time FTU form required |
| Static IP | Free | Free when attached to a running instance |
| Snapshots | ~$0.05/GB-month | 7 daily auto-snapshots |
| Data transfer overage | $0.09/GB | Unlikely with 4 TB allowance |
| **Total estimated** | **~$25–35/month** | |

> **Free trial**: New AWS accounts get 3 months free on smaller Lightsail bundles (2 GB). The 2 GB plan is below the recommended 4 GB but may work for testing.

## References

| Topic | URL |
|-------|-----|
| **Lightsail OpenClaw Quick Start** | https://docs.aws.amazon.com/lightsail/latest/userguide/amazon-lightsail-quick-start-guide-openclaw.html |
| Lightsail OpenClaw Announcement | https://aws.amazon.com/about-aws/whats-new/2026/03/amazon-lightsail-openclaw/ |
| AWS Blog Post | https://aws.amazon.com/blogs/aws/introducing-openclaw-on-amazon-lightsail-to-run-your-autonomous-private-ai-agents/ |
| OpenClaw Telegram Channel Docs | https://docs.openclaw.ai/channels/telegram |
| OpenClaw Configuration Reference | https://docs.openclaw.ai/gateway/configuration-reference |
| OpenClaw Security | https://docs.openclaw.ai/gateway/security/index |
| OpenClaw Health Checks | https://docs.openclaw.ai/gateway/health |
| Channel Pairing | https://docs.openclaw.ai/channels/pairing |
| Amazon Bedrock Model Access | https://docs.aws.amazon.com/bedrock/latest/userguide/model-access.html |
| Lightsail Pricing | https://aws.amazon.com/lightsail/pricing/ |
| Stakpak Autopilot Docs | https://stakpak.gitbook.io/docs |
| Telegram Bot API | https://core.telegram.org/bots/api |
