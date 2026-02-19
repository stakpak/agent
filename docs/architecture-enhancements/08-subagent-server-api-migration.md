# Design: Migrate Dynamic Subagents to Agent Server API

## Status

**Proposed** — 2026-02-18

## Summary

Replace the current CLI-process-spawning subagent mechanism with HTTP calls to the co-hosted Agent Server API. The master agent's MCP tools (`dynamic_subagent_task`, `resume_subagent_task`, `get_task_details`, etc.) will create server sessions instead of forking child processes, while preserving the full feature set visible to the LLM.

## Motivation

Dynamic subagents currently work by shelling out to the `stakpak` binary:

```
dynamic_subagent_task()
  → build CLI command string
  → TaskManager.start_task() spawns OS child process
  → child runs mode_async.rs agent loop
  → communicates via stdout JSON + exit codes
  → resume = spawn another child process with checkpoint flag
```

This works, but has concrete costs:

1. **Process overhead** — each subagent forks a new OS process (~100ms+), loads config, bootstraps MCP server, and initializes an inference client from scratch.
2. **Sandbox overhead** — sandboxed subagents spin up a Docker container *and* a CLI binary inside it (~5-10s).
3. **Fragile IPC** — communication relies on parsing stdout for `AsyncManifest` JSON and interpreting exit codes (0 = done, 10 = paused, other = failed).
4. **Resume cost** — resuming a paused subagent spawns an entirely new process that reloads the checkpoint from disk.
5. **No shared resources** — each child process creates its own inference client, MCP server, and tool registry. Memory scales linearly with subagent count.

The autopilot scheduler already migrated away from CLI spawning to the Agent Server API (`cli/src/commands/watch/agent.rs`). Subagents should follow the same path.

## Current Architecture

### Subagent Spawning Flow

```text
Master Agent (interactive or async mode)
  │
  │  LLM calls dynamic_subagent_task MCP tool
  ▼
subagent_tools.rs::dynamic_subagent_task()
  │
  ├─ validate tools array
  ├─ downgrade model (opus→haiku, sonnet→haiku)
  ├─ write prompt to file via LocalStore
  ├─ build CLI command string:
  │    stakpak -a --output json --prompt-file <path>
  │      --max-steps N --model <model> -t tool1 -t tool2
  │      [--pause-on-approval]
  │
  ├─ if sandbox: wrap in `stakpak warden wrap <image> -v ... -- <cmd>`
  │
  └─ TaskManager.start_task(command, description, timeout)
       │
       └─ spawns tokio::process::Command as child process
            │
            ├─ stdout captured line-by-line → task.output
            ├─ exit code 0  → TaskStatus::Completed
            ├─ exit code 10 → TaskStatus::Paused + parse AsyncManifest
            └─ other        → TaskStatus::Failed
```

### Resume Flow

```text
Master Agent calls resume_subagent_task MCP tool
  │
  ▼
subagent_tools.rs::resume_subagent_task()
  │
  ├─ look up task → extract checkpoint_id from pause_info
  ├─ build CLI command:
  │    stakpak -a --output json -c <checkpoint_id>
  │      [--approve tc_1] [--reject tc_2] [--prompt-file <input>]
  │
  └─ TaskManager.resume_task(task_id, command)
       │
       └─ spawns new child process (same as start_task)
```

### Monitoring Flow

```text
get_task_details(task_id)
  → TaskManager.get_task_details()
  → returns TaskInfo { status, output, pause_info }
  → if Paused: parse AsyncManifest from output for pending_tool_calls

get_all_tasks()
  → TaskManager.get_all_tasks()
  → returns Vec<TaskInfo> rendered as markdown table

wait_for_tasks(task_ids, timeout)
  → poll TaskManager in loop until all tasks reach terminal state

cancel_task(task_id)
  → TaskManager.cancel_task()
  → kills child process group
```

### Key Files

| File | Role |
|------|------|
| `libs/mcp/server/src/subagent_tools.rs` | `dynamic_subagent_task`, `resume_subagent_task` MCP tools |
| `libs/shared/src/task_manager.rs` | `TaskManager` — process spawning, tracking, cancellation |
| `libs/mcp/server/src/local_tools.rs` | `get_all_tasks`, `get_task_details`, `wait_for_tasks`, `cancel_task` |
| `cli/src/commands/agent/run/mode_async.rs` | Async agent loop (what the child process runs) |
| `cli/src/commands/agent/run/pause.rs` | `AsyncOutcome`, `ResumeInput`, exit code protocol |
| `libs/shared/src/models/async_manifest.rs` | `AsyncManifest`, `PauseReason`, `PendingToolCall` |

---

## Target Architecture

### Agent Server API (Already Exists)

The autopilot runtime hosts an Axum HTTP server (`libs/server/`) with a session-based agent API:

```text
HTTP Client
  │
  ▼
Axum routes (libs/server/src/routes.rs)
  ├─ POST /v1/sessions                          → create session
  ├─ POST /v1/sessions/{id}/messages             → start run / send follow-up
  ├─ GET  /v1/sessions/{id}/events               → SSE event stream
  ├─ GET  /v1/sessions/{id}/tools/pending        → get pending tool approvals
  ├─ POST /v1/sessions/{id}/tools/decisions      → batch approve/reject tools
  ├─ POST /v1/sessions/{id}/cancel               → cancel run
  ├─ GET  /v1/sessions/{id}                      → session detail + run status
  ├─ GET  /v1/sessions                           → list sessions
  └─ DELETE /v1/sessions/{id}                    → delete session

Session Actor (libs/server/src/session_actor.rs)
  ├─ loads checkpoint history
  ├─ builds AgentConfig (model, tools, approval policy)
  ├─ calls stakpak_agent_core::run_agent(...)
  ├─ forwards AgentEvent → EventLog → SSE
  └─ persists checkpoints periodically + on completion
```

### Proposed Subagent Flow

```text
Master Agent (interactive or server-hosted)
  │
  │  LLM calls dynamic_subagent_task MCP tool
  ▼
subagent_tools.rs::dynamic_subagent_task()   [REWRITTEN]
  │
  ├─ validate tools array
  ├─ downgrade model (opus→haiku, sonnet→haiku)
  ├─ combine context + instructions into user message
  │
  ├─ StakpakClient::create_session(title, parent_session_id, cwd)
  │    → POST /v1/sessions
  │    → returns session_id
  │
  ├─ StakpakClient::send_messages(session_id, message, opts)
  │    → POST /v1/sessions/{id}/messages
  │    → opts: { model, sandbox, max_steps, allowed_tools }
  │    → returns run_id
  │
  ├─ spawn background tokio task:
  │    subscribe to SSE events
  │    if pause_on_approval: break on ToolCallsProposed
  │    else: auto-approve all tool calls via resolve_tools()
  │    wait for RunCompleted / RunError
  │
  └─ register in SubagentTracker { session_id, run_id, status, ... }
       → return task_id to master agent
```

### Proposed Resume Flow

```text
Master Agent calls resume_subagent_task MCP tool
  │
  ▼
subagent_tools.rs::resume_subagent_task()   [REWRITTEN]
  │
  ├─ look up SubagentTracker → get session_id, run_id
  │
  ├─ if tool approval decisions:
  │    StakpakClient::resolve_tools(session_id, run_id, decisions)
  │      → POST /v1/sessions/{id}/tools/decisions
  │
  ├─ if follow-up input:
  │    StakpakClient::send_messages(session_id, input, { type: follow_up, run_id })
  │      → POST /v1/sessions/{id}/messages
  │
  └─ re-subscribe to SSE events (background task continues)
```

### Proposed Monitoring Flow

```text
get_task_details(task_id)
  → SubagentTracker.get(task_id)
  → StakpakClient::get_session(session_id)     → run status
  → StakpakClient::get_messages(session_id)     → latest output
  → StakpakClient::pending_tools(session_id)    → pause info
  → map to TaskInfo-compatible response

get_all_tasks()
  → SubagentTracker.list_all()
  → enrich each with session run_status
  → render as markdown table (same format as today)

wait_for_tasks(task_ids, timeout)
  → for each task: subscribe to SSE
  → tokio::select! across all streams + timeout
  → return when all reach terminal state

cancel_task(task_id)
  → SubagentTracker.get(task_id) → session_id, run_id
  → StakpakClient::cancel_run(session_id, run_id)
  → POST /v1/sessions/{id}/cancel
```

---

## Feature Gap Analysis

### What the Server API Already Supports

| Feature | Server API Mechanism |
|---------|---------------------|
| Create agent session | `POST /v1/sessions` |
| Start agent run | `POST /v1/sessions/{id}/messages` |
| Stream events | `GET /v1/sessions/{id}/events` (SSE) |
| Tool approval | `POST /v1/sessions/{id}/tools/decisions` |
| Cancel run | `POST /v1/sessions/{id}/cancel` |
| Sandbox isolation | `sandbox: true` in message request |
| Model selection | `model` field in message request |
| Checkpoint persistence | `CheckpointStore` + periodic flush |
| Follow-up messages | `type: follow_up` in message request |
| Steering messages | `type: steering` in message request |

### Gaps That Need Closing

#### Gap 1: Per-Session Tool Filtering

**Current behavior:** CLI `-t tool1 -t tool2` flags restrict which MCP tools the subagent can use. This is the "T" in the AOrchestra 4-tuple and is critical for least-privilege subagent design.

**Server today:** `session_actor` calls `state.current_mcp_tools()` which returns all registered tools. No per-session filtering.

**Required change:**

Add `allowed_tools: Option<Vec<String>>` to `SessionMessageRequest` in `routes.rs`. When present, `spawn_session_actor` filters the tool snapshot:

```rust
// session_actor.rs — inside run_session_actor()
let run_tools = if let Some(allowed) = allowed_tools {
    all_tools.into_iter()
        .filter(|t| allowed.iter().any(|a| tool_name_matches(&t.function.name, a)))
        .collect()
} else {
    all_tools
};
```

**Files to change:**
- `libs/server/src/routes.rs` — add field to `SessionMessageRequest`, pass through
- `libs/server/src/session_actor.rs` — accept `allowed_tools`, filter before `AgentConfig`
- `libs/gateway/src/client.rs` — add `allowed_tools` to `SendMessageOptions`

#### Gap 2: Per-Session Max Steps

**Current behavior:** `--max-steps N` controls the subagent's turn limit (default 30).

**Server today:** `MAX_TURNS = 64` is hardcoded in `session_actor.rs`.

**Required change:**

Add `max_steps: Option<usize>` to `SessionMessageRequest`. Pass through to `AgentConfig.max_turns`:

```rust
let agent_config = AgentConfig {
    max_turns: max_steps.unwrap_or(MAX_TURNS),
    // ...
};
```

**Files to change:**
- `libs/server/src/routes.rs` — add field to `SessionMessageRequest`
- `libs/server/src/session_actor.rs` — accept and use `max_steps`
- `libs/gateway/src/client.rs` — add `max_steps` to `SendMessageOptions`

#### Gap 3: Parent-Child Session Relationship

**Current behavior:** Subagent sessions are loosely linked via `--session-id` flag. The master agent tracks subagents through `TaskManager`'s in-memory HashMap keyed by task_id.

**Server today:** Sessions are independent entities with no parent-child relationship.

**Required change:**

Add `parent_session_id: Option<Uuid>` to `CreateSessionBody`. Store in session metadata so the master agent can query "list all my subagent sessions":

```rust
// routes.rs
struct CreateSessionBody {
    title: String,
    cwd: Option<String>,
    parent_session_id: Option<Uuid>,  // NEW
}
```

The `list_sessions` endpoint should support filtering by `parent_session_id` so `get_all_tasks` can efficiently list only the master's subagents.

**Files to change:**
- `libs/server/src/routes.rs` — add field to create/list handlers
- `libs/api/` — add `parent_session_id` to storage types and query filters
- `libs/gateway/src/client.rs` — add to `create_session`

#### Gap 4: Per-Session Tool Approval Policy

**Current behavior:** Non-sandboxed subagents use `--pause-on-approval` which pauses on mutating tools. Sandboxed subagents auto-approve everything (isolated environment).

**Server today:** Tool approval policy is global (`AppState.tool_approval_policy`), set at server startup.

**Required change:**

Add `auto_approve: Option<bool>` to `SessionMessageRequest`. When `true` (sandbox mode), override the session's approval policy to `ToolApprovalPolicy::All`:

```rust
let tool_approval = if auto_approve.unwrap_or(false) {
    ToolApprovalPolicy::All
} else {
    state.tool_approval_policy.clone()
};
```

This maps directly to the current sandbox vs non-sandbox behavior.

**Files to change:**
- `libs/server/src/routes.rs` — add field, pass through
- `libs/server/src/session_actor.rs` — accept per-session approval policy

#### Gap 5: Subagent Status Mapping

**Current behavior:** `TaskStatus` has 7 states: `Pending`, `Running`, `Completed`, `Failed`, `Cancelled`, `TimedOut`, `Paused`.

**Server today:** `SessionRuntimeState` has 4 states: `Idle`, `Starting`, `Running`, `Failed`.

**Required mapping:**

```text
SessionRuntimeState::Starting                          → Pending
SessionRuntimeState::Running + no pending_tools        → Running
SessionRuntimeState::Running + pending_tools present   → Paused (tool approval)
SessionRuntimeState::Idle + RunCompleted event          → Completed
SessionRuntimeState::Idle + RunError event              → Failed
SessionRuntimeState::Failed                            → Failed
Client-side timeout                                    → TimedOut
Cancel called                                          → Cancelled
```

This mapping lives entirely in the new subagent tools layer — no server changes needed.

#### Gap 6: Subagent Output Capture

**Current behavior:** `TaskManager` captures stdout line-by-line into `task.output`. The master agent reads this via `get_task_details`.

**Server today:** Agent output is available via `GET /v1/sessions/{id}/messages` (full message history) and SSE `TextDelta` events (streaming).

**Required approach:**

The background SSE listener task accumulates `TextDelta` events into a buffer. `get_task_details` returns this buffer as the "output" field. On completion, the full final assistant message is fetched via `get_messages`.

No server changes needed — this is client-side accumulation.

---

## Detailed Design

### New Component: `SubagentTracker`

Replaces `TaskManager`'s role for subagent tracking. Lives in `libs/mcp/server/src/` alongside the rewritten tools.

```rust
/// In-memory tracker for subagent sessions spawned by the master agent.
/// Maps short task_ids (for LLM compatibility) to server session state.
pub struct SubagentTracker {
    entries: HashMap<TaskId, SubagentEntry>,
}

pub struct SubagentEntry {
    pub task_id: TaskId,
    pub session_id: Uuid,
    pub run_id: Uuid,
    pub description: String,
    pub model: String,
    pub tools: Vec<String>,
    pub sandboxed: bool,
    pub start_time: DateTime<Utc>,
    pub status: SubagentStatus,
    pub output_buffer: String,
    pub event_listener: Option<JoinHandle<()>>,
}

pub enum SubagentStatus {
    Starting,
    Running,
    Paused { pending_tools: Vec<ProposedToolCall> },
    Completed,
    Failed { error: String },
    Cancelled,
    TimedOut,
}
```

### Rewritten `dynamic_subagent_task`

```rust
pub async fn dynamic_subagent_task(&self, ctx, params) -> Result<CallToolResult, McpError> {
    // 1. Validate
    if params.tools.is_empty() { return validation_error(); }

    // 2. Resolve model (downgrade opus/sonnet → haiku)
    let model = downgrade_model(ctx.meta.get("model_id"));

    // 3. Build user message (context + instructions)
    let user_message = build_subagent_prompt(&params.instructions, params.context.as_deref());

    // 4. Get server client
    let client = self.get_server_client()?;

    // 5. Create session
    let session = client.create_session(
        &params.description,
        Some(self.get_parent_session_id()),  // parent-child link
    ).await?;

    // 6. Send message to start run
    let run = client.send_messages(&session.id, vec![user_message], SendMessageOptions {
        model: Some(model),
        sandbox: if params.enable_sandbox { Some(true) } else { None },
        max_steps: params.max_steps,
        allowed_tools: Some(params.tools.clone()),
        auto_approve: if params.enable_sandbox { Some(true) } else { None },
        ..Default::default()
    }).await?;

    // 7. Spawn background event listener
    let task_id = generate_simple_id(6);
    let listener = spawn_event_listener(
        client.clone(),
        session.id,
        run.run_id,
        task_id.clone(),
        tracker.clone(),
        !params.enable_sandbox,  // pause_on_approval
    );

    // 8. Register in tracker
    tracker.insert(SubagentEntry {
        task_id: task_id.clone(),
        session_id: session.id,
        run_id: run.run_id,
        description: params.description,
        status: SubagentStatus::Running,
        event_listener: Some(listener),
        // ...
    });

    // 9. Return (same format as today)
    Ok(CallToolResult::success(format!(
        "🤖 Dynamic Subagent Created\n\nTask ID: {}\n...",
        task_id
    )))
}
```

### Background Event Listener

Each subagent gets a lightweight tokio task that subscribes to SSE events:

```rust
async fn event_listener_loop(
    client: StakpakClient,
    session_id: Uuid,
    run_id: Uuid,
    task_id: TaskId,
    tracker: Arc<RwLock<SubagentTracker>>,
    pause_on_approval: bool,
) {
    let mut events = client.subscribe_events(&session_id, None).await?;

    loop {
        let Some(event) = events.next_event().await? else { break };

        // Accumulate text output
        if let Some(delta) = event.as_text_delta() {
            tracker.append_output(&task_id, &delta).await;
        }

        // Handle tool approval
        if let Some(proposed) = event.as_tool_calls_proposed() {
            if pause_on_approval {
                tracker.set_paused(&task_id, proposed.tool_calls).await;
                break;  // stop listening until resume
            } else {
                // Auto-approve (sandbox mode)
                let decisions = auto_approve_all(&proposed.tool_calls);
                client.resolve_tools(&session_id, &run_id, decisions).await?;
            }
        }

        // Terminal events
        if event.as_run_completed().is_some() {
            tracker.set_completed(&task_id).await;
            break;
        }
        if let Some(err) = event.as_run_error() {
            tracker.set_failed(&task_id, err.error).await;
            break;
        }
    }
}
```

### Rewritten `resume_subagent_task`

```rust
pub async fn resume_subagent_task(&self, params) -> Result<CallToolResult, McpError> {
    let entry = tracker.get(&params.task_id)?;

    // Must be paused or completed
    if !matches!(entry.status, SubagentStatus::Paused { .. } | SubagentStatus::Completed) {
        return error("Task cannot be resumed");
    }

    let client = self.get_server_client()?;

    // Handle tool decisions
    if let SubagentStatus::Paused { ref pending_tools } = entry.status {
        let decisions = build_decisions(
            pending_tools,
            params.approve.as_deref(),
            params.reject.as_deref(),
            params.approve_all,
            params.reject_all,
        );
        client.resolve_tools(&entry.session_id, &entry.run_id, decisions).await?;
    }

    // Handle follow-up input
    if let Some(input) = params.input {
        client.send_messages(&entry.session_id, vec![user_message(input)], SendMessageOptions {
            message_type: MessageType::FollowUp,
            run_id: Some(entry.run_id),
            ..Default::default()
        }).await?;
    }

    // Re-spawn event listener
    tracker.set_running(&params.task_id).await;
    let listener = spawn_event_listener(/* ... */);
    tracker.set_listener(&params.task_id, listener).await;

    Ok(CallToolResult::success("🤖 Subagent Task Resumed\n..."))
}
```

### Rewritten Monitoring Tools

**`get_task_details`** — reads from `SubagentTracker`, enriches with live server state:

```rust
pub async fn get_task_details(&self, task_id) -> Result<CallToolResult, McpError> {
    let entry = tracker.get(&task_id)?;

    // Optionally refresh status from server
    if matches!(entry.status, SubagentStatus::Running) {
        let session = client.get_session(&entry.session_id).await?;
        let pending = client.pending_tools(&entry.session_id).await?;
        // update tracker if state changed
    }

    // Format output (same schema as today for LLM compatibility)
    Ok(format_task_details(&entry))
}
```

**`get_all_tasks`** — lists from tracker, same markdown table format.

**`wait_for_tasks`** — subscribes to SSE for each pending task, uses `tokio::select!` with timeout.

**`cancel_task`** — calls `POST /v1/sessions/{id}/cancel`, updates tracker.

---

## Server Client Access

The MCP tool container needs access to a `StakpakClient` pointing at the local server. Two options:

### Option A: Inject at MCP Server Init (Recommended)

When the autopilot starts, it knows the server URL and auth token. Pass a `StakpakClient` into the MCP server's `ToolContainer`:

```rust
// In autopilot.rs, after server starts:
let subagent_client = StakpakClient::new(loopback_url, auth_token);
// Pass to MCP server init
```

The `ToolContainer` already holds shared state (`task_manager`, `session_id`, etc.). Add `server_client: Option<Arc<StakpakClient>>`.

### Option B: Loopback Discovery

Tools discover the server URL from config at call time. More flexible but adds latency and failure modes.

**Recommendation:** Option A. The server URL and token are known at startup. No discovery needed.

### Fallback for Non-Server Contexts

When the agent runs in interactive mode without the server (no `stakpak up`), the `server_client` will be `None`. In this case, fall back to the current CLI-spawning behavior. This ensures subagents work in all contexts:

```rust
if let Some(client) = self.get_server_client() {
    // Server API path (new)
    spawn_via_server(client, params).await
} else {
    // CLI spawn path (existing, kept as fallback)
    spawn_via_cli(params).await
}
```

---

## Migration Phases

### Phase 1: Server-Side Enhancements

Add the missing fields to the server API. These are backward-compatible additions — no existing behavior changes.

| Change | File | Effort |
|--------|------|--------|
| `allowed_tools` on message request | `routes.rs`, `session_actor.rs` | Small |
| `max_steps` on message request | `routes.rs`, `session_actor.rs` | Small |
| `parent_session_id` on create session | `routes.rs`, storage types | Small |
| `auto_approve` on message request | `routes.rs`, `session_actor.rs` | Small |
| Filter `list_sessions` by parent | `routes.rs`, storage query | Small |
| Client SDK updates | `gateway/src/client.rs` | Small |

**Estimated effort:** 1-2 days

### Phase 2: SubagentTracker + Core Tools

Implement `SubagentTracker` and rewrite the two primary tools.

| Change | File | Effort |
|--------|------|--------|
| `SubagentTracker` struct | New file in `libs/mcp/server/src/` | Medium |
| Background event listener | New file or in tracker module | Medium |
| Rewrite `dynamic_subagent_task` | `subagent_tools.rs` | Medium |
| Rewrite `resume_subagent_task` | `subagent_tools.rs` | Medium |
| Inject `StakpakClient` into `ToolContainer` | `autopilot.rs`, MCP init | Small |
| CLI-spawn fallback path | `subagent_tools.rs` | Small |

**Estimated effort:** 3-4 days

### Phase 3: Monitoring Tools

Rewrite the monitoring/lifecycle tools to use `SubagentTracker` + server API.

| Change | File | Effort |
|--------|------|--------|
| Rewrite `get_task_details` | `local_tools.rs` | Small |
| Rewrite `get_all_tasks` | `local_tools.rs` | Small |
| Rewrite `wait_for_tasks` | `local_tools.rs` | Medium |
| Rewrite `cancel_task` | `local_tools.rs` | Small |

**Estimated effort:** 1-2 days

### Phase 4: Testing + Cleanup

| Change | Effort |
|--------|--------|
| Unit tests for `SubagentTracker` | Medium |
| Unit tests for status mapping | Small |
| Integration test: spawn + complete | Medium |
| Integration test: spawn + pause + resume | Medium |
| Integration test: spawn + cancel | Small |
| Integration test: CLI fallback path | Small |
| Remove dead code from old CLI-spawn path (keep fallback) | Small |
| Update AGENTS.md subagent documentation | Small |

**Estimated effort:** 2-3 days

**Total estimated effort:** 7-11 days

---

## MCP Tool Schema Compatibility

The LLM-facing tool schemas **must not change**. The `DynamicSubagentRequest` and `ResumeSubagentTaskRequest` structs keep their exact fields:

```rust
// NO CHANGES to these structs
pub struct DynamicSubagentRequest {
    pub description: String,
    pub instructions: String,
    pub context: Option<String>,
    pub tools: Vec<String>,
    pub max_steps: Option<usize>,
    pub enable_sandbox: bool,
}

pub struct ResumeSubagentTaskRequest {
    pub task_id: String,
    pub approve: Option<Vec<String>>,
    pub reject: Option<Vec<String>>,
    pub approve_all: Option<bool>,
    pub reject_all: Option<bool>,
    pub input: Option<String>,
}
```

The tool descriptions, parameter schemas, and return value formats remain identical. The migration is invisible to the LLM.

---

## Benefits

| Aspect | Before (CLI spawn) | After (Server API) |
|--------|--------------------|--------------------|
| **Spawn latency** | ~100ms+ (fork + init) | ~5ms (HTTP + tokio task) |
| **Sandbox latency** | ~5-10s (Docker + CLI) | ~3-5s (Docker only) |
| **Memory per subagent** | Full process (inference client, MCP server, tool registry) | Shared inference client and MCP server |
| **Resume** | Fork new process, reload checkpoint from disk | Single HTTP request, continue in-memory |
| **IPC** | stdout JSON parsing + exit codes | Typed HTTP responses + SSE events |
| **Observability** | Parse `AsyncManifest` from stdout | Structured `AgentEvent` stream |
| **Persistence** | File-based `AsyncManifest` | SQLite checkpoints via `CheckpointStore` |
| **Concurrency ceiling** | OS process limits | Tokio task limits (orders of magnitude higher) |
| **Error handling** | Exit code interpretation | Typed `ClientError` + `AgentEvent::RunError` |

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| **Server not running** when subagent tools are called (interactive mode without `stakpak up`) | Subagents would fail | CLI-spawn fallback path (Phase 2). Detect `server_client == None` and use existing behavior. |
| **Tool filtering regression** — subagent gets more tools than intended | Security: subagent could execute unintended operations | Unit tests comparing filtered tool lists. The `allowed_tools` filter is a simple name match — same logic as CLI's `--allowed-tools`. |
| **SSE connection drops** mid-run | Subagent appears stuck | Event listener reconnects with `Last-Event-ID`. Server's `EventLog` supports replay with gap detection. |
| **Checkpoint format incompatibility** | Resume fails across old/new | Both paths use `CheckpointEnvelopeV1` — same format. Server and CLI share `stakpak-agent-core`. |
| **TaskManager still needed** for `run_command_task` | Can't remove TaskManager entirely | Keep `TaskManager` for non-agent background commands (`run_command_task`). Only subagent tools migrate. |
| **Race condition** — master queries status while event listener is updating tracker | Stale status returned | `SubagentTracker` uses `Arc<RwLock<...>>`. Status reads are eventually consistent (acceptable — same as current `TaskManager`). |
| **Server resource exhaustion** — many concurrent subagent sessions | Memory/CPU pressure | `max_steps` limits turn count. Server already handles concurrent sessions. Consider adding a max-concurrent-subagents config if needed. |

---

## What Does NOT Change

- **`TaskManager`** — stays for `run_command_task` (non-agent background shell commands)
- **`mode_async.rs`** — stays as the CLI async agent loop (used by fallback path and standalone `-a` mode)
- **`AsyncManifest` / pause protocol** — stays for CLI-mode compatibility
- **MCP tool schemas** — identical from the LLM's perspective
- **`agent-core::run_agent`** — the canonical agent loop is already shared between server and CLI
- **Sandbox container isolation** — server already supports `SandboxConfig` per session

---

## Open Questions

1. **Max concurrent subagents** — should we enforce a limit? The current `TaskManager` has no limit. The server has no limit either. For safety, consider a configurable cap (e.g., 10 concurrent subagent sessions).

2. **Session cleanup** — subagent sessions accumulate in storage. Should we auto-delete completed subagent sessions after the parent session ends? Or keep them for audit/debugging?

3. **Model override** — currently the master agent's model is downgraded (opus→haiku, sonnet→haiku). Should this be configurable per-subagent, or should the server enforce a "subagent model tier" policy?

4. **Interactive mode server** — should `stakpak` (interactive TUI mode) also start a lightweight embedded server to enable server-API subagents without requiring `stakpak up`? This would eliminate the need for the CLI-spawn fallback entirely.

---

## References

- `libs/server/README.md` — server architecture and API surface
- `libs/agent-core/README.md` — canonical agent loop documentation
- `cli/src/commands/watch/agent.rs` — existing server API integration (scheduler)
- `libs/gateway/src/client.rs` — `StakpakClient` HTTP client
- `docs/architecture-enhancements/03-http-server-api.md` — original server API proposal
