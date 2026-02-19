# stakpak-gateway

Messaging gateway runtime for `stakpak`.

It bridges chat platforms (Telegram / Slack / Discord) to the Stakpak server API (`/v1/sessions/...`), manages routing/session mapping, and exposes a small Gateway API for outbound messages and autopilot notifications.

---

## What it does

- Receives inbound messages from channels
- Routes each conversation to a stable Stakpak session
- Sends user messages to the autopilot server runtime
- Streams run events and returns assistant replies back to channel
- Handles tool decisions using configured approval policy
- Stores routing/session mappings in SQLite
- Supports autopilot notifications via `POST /v1/gateway/send`

---

## Architecture & data flow

```text
Channel adapter (telegram/slack/discord)
  └─ emits InboundMessage
        │
        ▼
Dispatcher
  ├─ resolve routing key (dm/group/thread)
  ├─ load/create session mapping (GatewayStore)
  ├─ send user message to autopilot server
  ├─ subscribe to SSE events for the run
  ├─ auto-resolve tool approvals (gateway policy)
  └─ deliver assistant reply back to channel
        │
        ▼
Channel adapter send(...)
```

### Core components

- **`runtime.rs`**
  - boots channels + dispatcher + prune loop
  - mounts gateway API state
- **`dispatcher.rs`**
  - main orchestration loop for inbound messages and run events
  - queues follow-up messages while a run is active for a session
  - keeps per-session SSE cursor to resume safely
- **`router.rs`**
  - computes stable routing keys for direct/group/thread conversations
  - supports bindings + DM scope behavior
- **`store.rs`**
  - SQLite persistence for routing key → session mapping
  - stores one-shot `delivery_context` for autopilot notification replies
- **`client.rs`**
  - HTTP + SSE client to autopilot server
  - sends messages, receives run events, resolves tool decisions
- **`api.rs`**
  - gateway HTTP surface:
    - `GET /status`
    - `GET /channels`
    - `GET /sessions`
    - `POST /send`

### Session model

- Each chat target resolves to a **routing key**.
- Routing key maps to one persistent Stakpak **session_id**.
- For thread-aware channels, each thread can map to a separate session.
- Delivery metadata is refreshed on inbound messages so replies go to the right target.

### Tool approval model

- Gateway receives `tool_calls_proposed` from SSE.
- It builds decisions using configured policy (`allow_all`, `deny_all`, `allowlist`).
- In autopilot mode, approval policy is derived from the profile's auto-approve settings.

---

## How to run

The gateway is managed through the autopilot system. There are no standalone `stakpak gateway` commands.

### Setup channels

```bash
# Add channels via autopilot CLI
stakpak autopilot channel add slack --bot-token "$SLACK_BOT_TOKEN" --app-token "$SLACK_APP_TOKEN"
stakpak autopilot channel add telegram --token "$TELEGRAM_BOT_TOKEN"
stakpak autopilot channel add discord --token "$DISCORD_BOT_TOKEN"

# Verify channels
stakpak autopilot channel list
stakpak autopilot channel test
```

### Start autopilot (includes gateway)

```bash
stakpak up
```

This starts the full autopilot runtime which includes:
- **Scheduler** — cron-based schedule execution
- **Server** — HTTP API on `127.0.0.1:4096`
- **Gateway** — channel adapters + routing

Gateway routes are available at `http://127.0.0.1:4096/v1/gateway/*`.

### Channel management

```bash
stakpak autopilot channel list
stakpak autopilot channel test
stakpak autopilot channel add <type> --token ...
stakpak autopilot channel remove <type>
```

Configuration is stored in `~/.stakpak/autopilot.toml`.

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

## Slack setup

### Behavior

- DMs always work
- In channels: bot responds when mentioned
- Thread sessions are supported
- Receipt reaction (`:eyes:`) is added on accepted inbound messages

### Required Bot Token Scopes

The Slack bot needs specific OAuth scopes. Without the read/history scopes, the bot can send notifications but **cannot receive inbound messages**.

| Scope | Purpose | Required for |
|-------|---------|-------------|
| `chat:write` | Send messages to channels | Outbound (notifications) |
| `reactions:read` | Read emoji reactions | Inbound |
| `reactions:write` | Add emoji reactions (`:eyes:` receipt) | Outbound |
| `channels:read` | See public channels the bot is in | Inbound |
| `groups:read` | See private channels the bot is in | Inbound |
| `im:read` | See DM conversations | Inbound |
| `mpim:read` | See group DM conversations | Inbound |
| `channels:history` | Read messages in public channels | Inbound |
| `groups:history` | Read messages in private channels | Inbound |
| `im:history` | Read DM messages | Inbound |
| `mpim:history` | Read group DM messages | Inbound |
| `app_mentions:read` | Receive @mention events | Inbound |

Socket Mode also requires an **App-Level Token** (`xapp-*`) with the `connections:write` scope.

### Slack App Configuration

1. Go to [api.slack.com/apps](https://api.slack.com/apps) → select the app
2. **OAuth & Permissions** → add all Bot Token Scopes listed above
3. **Socket Mode** → enable (requires App-Level Token / `xapp-*`)
4. **Event Subscriptions** → enable and subscribe to bot events:
   - `message.channels` — messages in public channels
   - `message.groups` — messages in private channels
   - `message.im` — direct messages
   - `app_mention` — @mentions
5. **Reinstall the app** to the workspace (scope changes require reinstall)
6. Update `autopilot.toml` with the new `xoxb-*` bot token (or re-run `stakpak autopilot channel add slack ...`)

### Troubleshooting

If `stakpak autopilot channel test` passes (✓) but the bot never responds to messages:

```bash
# Check if bot has read scopes
curl -s https://slack.com/api/users.conversations \
  -H "Authorization: Bearer $SLACK_BOT_TOKEN" | jq .error
# "missing_scope" → bot only has chat:write, needs read scopes above
```

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
