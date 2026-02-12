# stakpak-gateway

Messaging gateway runtime for `stakpak`.

It bridges chat platforms (Telegram / Slack / Discord) to the Stakpak server API (`/v1/sessions/...`), manages routing/session mapping, and exposes a small Gateway API for outbound messages and watch notifications.

---

## What it does

- Receives inbound messages from channels
- Routes each conversation to a stable Stakpak session
- Sends user messages to `stakpak serve`
- Streams run events and returns assistant replies back to channel
- Handles tool decisions using configured approval policy
- Stores routing/session mappings in SQLite
- Supports watch notifications via `POST /v1/gateway/send`

---

## Recommended way to run

### 1) Create gateway config

```bash
stakpak gateway init --force
```

Or non-interactive:

```bash
stakpak gateway init \
  --telegram-token "$TELEGRAM_BOT_TOKEN" \
  --discord-token "$DISCORD_BOT_TOKEN" \
  --slack-bot-token "$SLACK_BOT_TOKEN" \
  --slack-app-token "$SLACK_APP_TOKEN" \
  --force
```

Config is saved at:

`~/.stakpak/gateway.toml`

---

### 2) Validate configured channels

```bash
stakpak gateway channels list
stakpak gateway channels test
```

---

### 3) Start with server integration (recommended)

```bash
stakpak serve --gateway --no-auth
```

This runs:
- server on `127.0.0.1:4096`
- gateway runtime inside serve
- gateway routes at `http://127.0.0.1:4096/v1/gateway/*`

---

## Alternative: run gateway standalone

Terminal 1:

```bash
stakpak serve --no-auth
```

Terminal 2:

```bash
stakpak gateway run
```

Standalone gateway API defaults to:

`http://127.0.0.1:4097/v1/gateway/*`

---

## Gateway API quick test

### Status

```bash
curl -s http://127.0.0.1:4096/v1/gateway/status
```

### Send outbound message

```bash
curl -X POST http://127.0.0.1:4096/v1/gateway/send \
  -H 'Content-Type: application/json' \
  -d '{
    "channel": "slack",
    "target": { "channel": "C1234567890" },
    "text": "Hello from gateway"
  }'
```

Channel target formats:
- Telegram: `{ "chat_id": "...", "thread_id": "..." }`
- Discord: `{ "channel_id": "...", "thread_id": "...", "message_id": "..." }`
- Slack: `{ "channel": "...", "thread_ts": "..." }`

---

## Useful CLI commands

```bash
# Create/update config
stakpak gateway init --force

# Channel management
stakpak gateway channels list
stakpak gateway channels test
stakpak gateway channels add --channel slack
stakpak gateway channels remove --channel discord

# Run gateway alone
stakpak gateway run
stakpak gateway run --url http://127.0.0.1:4096 --bind 127.0.0.1:4097

# Run everything
stakpak up
```

---

## Approval behavior (important)

When running through:

```bash
stakpak serve --gateway ...
```

gateway tool decisions follow serve/profile auto-approve settings.

- `--auto-approve-all` => gateway allow all
- profile `auto_approve` allowlist => gateway allowlist

When running standalone (`stakpak gateway run`), approvals are taken from `gateway.toml`.

---

## Slack behavior

- DMs always work
- In channels: bot responds when mentioned
- Thread sessions are supported
- Receipt reaction (`:eyes:`) is added on accepted inbound messages

Make sure Slack bot scopes include at least:
- `chat:write`
- `reactions:write`
- `channels:history`, `groups:history`, `im:history`, `mpim:history`
- Socket Mode app token (`connections:write`)

---

## Library usage (Rust)

```rust
use stakpak_gateway::{Gateway, GatewayCliFlags, GatewayConfig};
use tokio_util::sync::CancellationToken;

# async fn run() -> anyhow::Result<()> {
let cli = GatewayCliFlags {
    url: Some("http://127.0.0.1:4096".into()),
    token: Some("".into()),
    ..Default::default()
};

let config = GatewayConfig::load_default(&cli)?;
let gateway = Gateway::new(config)?;

let cancel = CancellationToken::new();
gateway.run(cancel).await?;
# Ok(())
# }
```

---

## Source layout

- `src/runtime.rs` – Gateway runtime boot + channel wiring
- `src/dispatcher.rs` – inbound -> server run -> outbound reply loop
- `src/client.rs` – Stakpak HTTP/SSE client
- `src/store.rs` – SQLite mapping/context store
- `src/router.rs` – routing key and scope resolution
- `src/targeting.rs` – outbound target parsing + keying
- `src/channels/*` – Telegram/Slack/Discord implementations
- `src/api.rs` – `/v1/gateway/{status,channels,sessions,send}`
