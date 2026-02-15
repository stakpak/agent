# stakpak-server

HTTP/SSE runtime shell around `stakpak-agent-core`.

This crate owns session/run orchestration, API contracts, durable event streaming, checkpoint persistence, auth boundaries, and MCP tool bridge wiring.

## What this crate owns

- REST + SSE API surface (Axum)
- Session runtime state machine (`SessionManager`)
- Per-run actor orchestration (`session_actor`)
- Durable per-session event log with replay (`EventLog`)
- Idempotency handling for mutating endpoints
- Checkpoint persistence envelope store
- MCP tool execution bridge (through `stakpak-mcp-client`)
- Runtime config exposure (`/v1/config` and `session.config`)

## What this crate does **not** own

- Core agent loop logic (delegated to `stakpak-agent-core::run_agent`)
- Provider/client bootstrap policy from CLI profile resolution
- CLI/TUI UI behavior

---

## High-level architecture

```text
HTTP client
  │
  ▼
Axum routes (routes.rs)
  ├─ auth boundary (public vs protected routers)
  ├─ request validation + idempotency
  ├─ SessionManager (start/cancel/command/run scope)
  └─ spawn_session_actor(...)

session_actor
  ├─ load checkpoint/messages
  ├─ build AgentConfig + tool snapshot
  ├─ run stakpak_agent_core::run_agent(...)
  ├─ forward AgentEvent -> EventLog
  └─ persist checkpoints (periodic + lifecycle hooks)

EventLog
  ├─ monotonic event ids per session
  ├─ replay ring buffer
  └─ live broadcast stream (SSE)
```

---

## Main components

- `routes.rs` — all HTTP handlers and SSE stream endpoint
- `session_manager.rs` — run-scoped state machine (`Idle|Starting|Running|Failed`)
- `session_actor.rs` — adapter from server runtime to `run_agent`
- `event_log.rs` — durable replay + live pub/sub
- `checkpoint_store.rs` — latest checkpoint envelope storage
- `idempotency.rs` — `Idempotency-Key` lookup/replay/conflict logic
- `message_bridge.rs` — temporary storage edge adapters (`ChatMessage` <-> `stakai::Message`)
- `auth.rs` — bearer auth middleware
- `openapi.rs` — generated OpenAPI document definitions
- `state.rs` — shared `AppState` and MCP tool cache helpers

---

## Run lifecycle

1. Client calls `POST /v1/sessions/{id}/messages` with a `stakai::Message`.
2. Route validates request and run-scope constraints.
3. `SessionManager::start_run` atomically reserves `Starting { run_id }`.
4. Actor boots and transitions state to `Running { run_id, handle }`.
5. Actor calls `run_agent` from `stakpak-agent-core`.
6. Core events are published to `EventLog` and exposed via SSE.
7. Checkpoints are flushed periodically and on terminal transitions.
8. `SessionManager::mark_run_finished` moves runtime state back to `Idle` or `Failed`.

---

## API and runtime guarantees

- **Run-scoped safety**: run-mismatch returns conflict (prevents stale commands).
- **Idempotency**: mutating endpoints support `Idempotency-Key` replay semantics.
- **Durable events**: replay from `Last-Event-ID`, with `gap_detected` control event when cursor is out of ring window.
- **Deterministic tooling per run**: tools are snapshotted for the run; refresh affects new runs.
- **StakAI-native boundary**: session message input/output is `stakai::Message`.
- **Runtime config visibility**: session detail responses include a `config` object with effective runtime snapshot fields like:
  - `default_model`
  - `auto_approve_mode`

---

## Public vs protected routes

- Public:
  - `GET /v1/health`
  - `GET /v1/openapi.json`
- Protected:
  - all `/v1/sessions/*`, `/v1/models`, `/v1/config`, etc.

Auth middleware is applied only to the protected router.

---

## Running the server (operator quickstart)

The server runs as part of the autopilot runtime:

```bash
# Start full autopilot (server + gateway + scheduler)
stakpak up
```

Default bind is `127.0.0.1:4096`.

If auth is enabled, provide a bearer token (`Authorization: Bearer <token>`) for protected routes.

### Useful endpoints while serving

- `GET /v1/health`
- `GET /v1/openapi.json`
- `POST /v1/sessions`
- `POST /v1/sessions/{id}/messages`
- `GET /v1/sessions/{id}/events` (SSE)
- `GET /v1/sessions/{id}/messages`
- `POST /v1/sessions/{id}/cancel`

When gateway is enabled, these are also mounted:

- `GET /v1/gateway/status`
- `GET /v1/gateway/channels`
- `GET /v1/gateway/sessions`
- `POST /v1/gateway/send`

## Channel-related CLI commands

Channels are managed through the autopilot system:

```bash
# Add channels
stakpak autopilot channel add slack --bot-token "$SLACK_BOT_TOKEN" --app-token "$SLACK_APP_TOKEN"
stakpak autopilot channel add telegram --token "$TELEGRAM_BOT_TOKEN"
stakpak autopilot channel add discord --token "$DISCORD_BOT_TOKEN"

# Validate configured channels
stakpak autopilot channel list
stakpak autopilot channel test

# Start everything (server + gateway + scheduler)
stakpak up
```

## Relationship to core

`stakpak-server` should be treated as a runtime adapter + API shell.
All agent behavior (turn loop, approvals, retries, compaction, event semantics) should remain centralized in `stakpak-agent-core`.
