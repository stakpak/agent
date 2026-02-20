# stakpak-agent-core

Canonical agent runtime loop for Stakpak.

This crate is intentionally transport-agnostic: it has no HTTP, no TUI, and no storage backend assumptions. It focuses on deterministic multi-turn execution, tool orchestration, retries/compaction, and typed runtime events.

## What this crate owns

- Multi-turn run loop (`run_agent`)
- Context reduction and tool-call/result hygiene
- Tool approval state machine (single + bulk decisions)
- Tool execution contract via `ToolExecutor`
- Retry policy and backoff helpers
- Context overflow compaction contract via `CompactionEngine`
- Versioned checkpoint envelope helpers
- Typed lifecycle/events for host runtimes

## What this crate does **not** own

- HTTP/SSE transport
- Session persistence implementation
- MCP bootstrapping/wiring
- Auth or API semantics

---

## High-level architecture

```text
Host runtime (server / local runtime)
  ├─ provides run_id + session_id (AgentRunContext)
  ├─ provides model/config + initial history
  ├─ provides ToolExecutor + AgentHook(s) + CompactionEngine
  ├─ sends AgentCommand over mpsc
  └─ receives AgentEvent stream

                │
                ▼
          run_agent(...)
                │
      ┌─────────┴─────────┐
      │ Per-turn execution │
      └─────────┬─────────┘
                │
        reduce_context()
                │
        inference.generate()
                │
     parse text/reasoning/tool calls
                │
    ApprovalStateMachine (policy + user)
                │
        ToolExecutor::execute_tool_call
                │
         append tool_result messages
                │
     retry/compaction as needed
                │
         emit AgentEvent updates
```

---

## Core API

- Entry point: `run_agent` (re-exported from `lib.rs`)
- Primary config: `AgentConfig`
- Run identity: `AgentRunContext { run_id, session_id }`
- Runtime control: `AgentCommand`
- Runtime telemetry: `AgentEvent`
- Result: `AgentLoopResult`

`run_id` is always provided by the host runtime (this crate never invents one).

## Extension points

### `ToolExecutor`
Host provides how tool calls are actually executed (e.g. through MCP):

- input: `AgentRunContext`, `ProposedToolCall`, `CancellationToken`
- output: `ToolExecutionResult::{Completed|Cancelled}`

### `AgentHook`
Hook into lifecycle stages:

- `before_inference`
- `after_inference`
- `before_tool_execution`
- `after_tool_execution`
- `on_error`

Typical use: checkpoint persistence, telemetry, custom audits.

### `CompactionEngine`
When context overflow is detected and compaction is enabled, `run_agent` delegates message compaction to the host-provided engine.

---

## Internal module map

- `agent.rs` — main loop (`run_agent`)
- `types.rs` — configs, commands, events, loop result
- `approval.rs` — deterministic approval FSM
- `context.rs` — reduction pipeline and structural cleanup
- `tools.rs` — tool execution trait + result types
- `retry.rs` — delay parsing and backoff helpers
- `compaction.rs` — compaction contract
- `checkpoint.rs` — `CheckpointEnvelopeV1` serialize/deserialize + migration
- `stream.rs` — ordered stream assembly helpers
- `hooks.rs` — hook trait
- `error.rs` — typed runtime errors

---

## Key invariants

- Every assistant-declared tool call must end with exactly one terminal decision/result path.
- Tool decisions can arrive out-of-order, but execution remains in original declaration order.
- Context cleanup removes duplicate/orphaned tool results and strips dangling tool calls.
- Retry attempts are bounded by `RetryConfig`.
- Overflow handling uses compaction (when enabled) instead of blindly retrying.

---

## Where this is used

- `libs/server` (`stakpak-server`) uses this crate for remote HTTP/SSE runs.
- Local runtime surfaces should use this same engine for behavioral parity.
