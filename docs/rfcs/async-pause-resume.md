# RFC: Async Agent Pause and Resume

**Status:** Draft  
**Author:** Stakpak Team  
**Created:** 2026-02-06  
**Last Updated:** 2026-02-06

## Summary

This RFC proposes a mechanism for headless async agents to pause execution when they require human input or tool approval, and resume execution when that input is provided. The design enables agents running in CI/CD pipelines, as subagents, or on remote servers to participate in human-in-the-loop workflows without requiring persistent connections or long-running blocked processes.

## Motivation

### Current State

Today, the Stakpak agent has two modes:

1. **Interactive mode (TUI)**: Full human-in-the-loop support via the approval bar. Users can approve/reject individual tool calls, provide input, and control execution in real-time.

2. **Async mode (headless)**: Fire-and-forget execution. The agent runs until completion or error, auto-executing all tool calls without any approval gates. There's no way to pause for human input.

This creates a gap: headless agents in CI/CD, running as subagents, or deployed on remote servers cannot participate in approval workflows. They either run with full autonomy (risky for destructive operations) or don't run at all.

### Use Cases

1. **CI/CD with approval gates**: An agent deploying to production should pause before running `terraform apply` and wait for human approval via Slack, GitHub PR comment, or a webhook.

2. **Subagent coordination**: A parent agent spawns a child agent for a task. The child needs credentials or clarification. Instead of failing, it pauses and the parent (or human) provides the input.

3. **Long-running autonomous tasks**: An agent working on a multi-hour task encounters an ambiguous situation. It pauses, asks for clarification, and resumes when guidance is provided.

4. **Audit and compliance**: Certain operations require explicit human approval for compliance reasons. The agent must stop, record the pending action, and wait for authorized approval.

## Design Principles

### 1. Process Lifecycle as State Machine

The agent process dying IS the pause notification. No long-running blocked processes, no websockets, no polling. The agent checkpoints its state, exits with a distinct code, and a new process resumes later.

```
RUNNING ──→ PAUSED (exit 10) ──→ RESUMED (new process)
   │                                      │
   └──→ COMPLETED (exit 0)                └──→ RUNNING ──→ ...
   └──→ FAILED (exit 1)
```

**Reasoning**: Processes are universal. Every orchestration system (CI/CD, cron, systemd, Kubernetes Jobs, parent processes) understands exit codes. This requires no new infrastructure.

### 2. Checkpoints as the Source of Truth

All state needed to resume lives in the checkpoint. The checkpoint already stores the full message history. We extend it to also store pending tool calls when pausing for approval.

**Reasoning**: Checkpoints already exist and work. Resume via `stakpak -c <checkpoint_id>` is already implemented. We're extending an existing mechanism, not inventing a new one.

### 3. Dual Control Plane

Everything must be controllable by both humans and agents. The same resume operation works whether triggered by:
- A parent agent calling an MCP tool
- A human using the CLI
- A CI/CD system via webhook
- The TUI (for interactive mode)

**Reasoning**: Agents and humans are peers in the system. A parent agent supervising subagents needs the same control primitives as a human operator.

### 4. Task Manager Stays Generic

The task manager handles process lifecycle. It knows that exit code 10 means "paused" and that paused tasks can be resumed. It does NOT parse agent-specific JSON or understand tool calls.

**Reasoning**: The task manager is shared infrastructure used for both simple background commands (`run_command_task`) and agent subprocesses (`subagent_task`). Keeping it generic maintains clean separation of concerns.

## Detailed Design

### Pause Triggers

There are exactly two situations where an async agent pauses:

#### Trigger 1: Tool Approval Required

The LLM returned tool calls, and one or more are flagged as requiring approval in the `AutoApproveConfig`. The agent pauses BEFORE executing any tools from that batch.

**Current behavior**: Async mode ignores `AutoApproveConfig` and executes all tools.

**New behavior**: Async mode respects `AutoApproveConfig`. When tools require approval, the agent:
1. Saves a checkpoint (the pending tool calls are already in the last assistant message's `tool_calls` field)
2. Writes a pause manifest
3. Exits with code 10

#### Trigger 2: Input Required

The LLM responded with text only (no tool calls). This typically means the agent is asking a question, reporting a blocker, or requesting clarification.

**Current behavior**: Async mode treats this as "completed successfully" and exits.

**New behavior**: This is recognized as a pause point. The agent:
1. Saves a checkpoint
2. Writes a pause manifest with the agent's message
3. Exits with code 10

**Note**: We do NOT add a `request_human_input` tool. If the LLM needs input, it simply responds with text and no tool calls. This is the natural way LLMs express "I need information."

### Why These Are Mutually Exclusive

LLM APIs enforce a strict message ordering:
- If an assistant message contains `tool_calls`, the subsequent messages must be `role: tool` results
- A `role: user` message cannot be inserted between tool calls and their results

Therefore:
- **Tool approval pause**: Resume provides tool results (approved → executed, rejected → TOOL_CALL_REJECTED)
- **Input required pause**: Resume provides a user message

These cannot happen simultaneously. The resume input is either tool decisions OR a text prompt, never both.

### Exit Codes

| Code | Meaning | Task Status |
|------|---------|-------------|
| 0 | Completed successfully | `Completed` |
| 1 | Error/failure | `Failed` |
| 10 | Paused (needs input or approval) | `Paused` |

**Reasoning**: A single pause code keeps it simple. The pause manifest contains the details (which type of pause, what's pending). Orchestrators only need to check for code 10 to know "this needs attention."

### Output Contract

#### stdout (JSON mode: `--output json`)

On pause (exit 10):
```json
{
  "outcome": "paused",
  "checkpoint_id": "abc-123",
  "session_id": "def-456",
  "pause_reason": {
    "type": "tool_approval_required",
    "pending_tool_calls": [
      {
        "id": "tc_1",
        "name": "run_command",
        "arguments": {"command": "terraform apply"}
      }
    ]
  },
  "agent_message": "I'm about to apply the terraform changes to production.",
  "resume_hint": "stakpak -c abc-123 --approve tc_1"
}
```

On completion (exit 0):
```json
{
  "outcome": "completed",
  "checkpoint_id": "abc-123",
  "session_id": "def-456",
  "final_message": "Deployment complete.",
  "steps_taken": 12
}
```

**Reasoning**: stdout is the API. Machine consumers parse JSON. The `resume_hint` provides a copy-pasteable command for simple cases.

#### stderr

Operational logs, progress indicators, warnings. Only emitted when `--verbose` is set. Humans watching the process see progress here; machines ignore it.

#### Pause Manifest File

`.stakpak/session/pause.json` — Written on exit 10, contains the same data as stdout JSON.

**Reasoning**: Redundant with stdout but useful for:
- Shell scripts that don't capture stdout
- CI systems that check for file existence
- Debugging (file persists after process exits)
- Cases where stdout is redirected/lost

### Resume Mechanism

#### CLI Flags (Async Mode)

New flags for `stakpak` when resuming from a checkpoint:

```bash
# Resume with tool approvals
stakpak -c <checkpoint_id> --approve <tool_call_id> [--approve <tool_call_id>...]
stakpak -c <checkpoint_id> --reject <tool_call_id> [--reject <tool_call_id>...]

# Approve all pending tools
stakpak -c <checkpoint_id> --approve-all

# Reject all pending tools  
stakpak -c <checkpoint_id> --reject-all

# Resume with text input (existing behavior, now explicit)
stakpak -c <checkpoint_id> "your input here"

# Mixed: some approved, some rejected
stakpak -c <checkpoint_id> --approve tc_1 --approve tc_2 --reject tc_3
```

**Reasoning**: CLI flags are explicit and scriptable. `--approve` and `--reject` take tool call IDs from the pause manifest. This matches how humans think: "approve this one, reject that one."

#### Resume Logic

When resuming from a paused checkpoint:

1. Load checkpoint (messages + pending_tool_calls)
2. Parse CLI flags to determine decisions
3. For tool approval pause:
   - Execute approved tools, collect results
   - For rejected tools, inject `TOOL_CALL_REJECTED` as the tool result
   - Continue the async loop
4. For input required pause:
   - Inject the prompt as a user message
   - Continue the async loop

### Task Manager Extensions

#### New Status

```
TaskStatus::Paused
```

A task in `Paused` status:
- Has exited (no running process)
- Has a stored checkpoint_id and raw output
- Can be resumed via `resume_task`
- Retains the same task ID across resume cycles

#### New Operation

```
resume_task(task_id, resume_input) -> Result<(), TaskError>
```

Where `resume_input` is an opaque string (JSON) that the task manager passes through to the resume command.

The task manager:
1. Looks up the paused task
2. Extracts checkpoint_id from stored pause info
3. Constructs resume command: `stakpak -c {checkpoint_id} {resume_flags_from_input}`
4. Spawns new process
5. Reuses the same task ID
6. Sets status back to `Running`

**Reasoning**: The task manager doesn't parse agent-specific JSON. It just knows "this task is paused, here's how to resume it." The translation from structured resume input to CLI flags happens in a helper layer.

#### Task Continuity

The key insight: **task ID is the stable handle** across process invocations.

```
Task abc123:
  Invocation 1: stakpak -a "deploy" → exit 10 (paused)
  Invocation 2: stakpak -c xyz --approve tc_1 → exit 10 (paused again)
  Invocation 3: stakpak -c xyz2 "yes, proceed" → exit 0 (completed)
```

All three invocations are the same logical task. The parent agent or human sees one task ID throughout.

### MCP Tool Interface

New tool for parent agents:

```
resume_task(
  task_id: String,
  tool_decisions: Option<[{tool_call_id: String, approved: bool}]>,
  prompt: Option<String>
)
```

Only one of `tool_decisions` or `prompt` should be provided (enforced by validation).

**Reasoning**: Parent agents need a typed interface, not string manipulation. The MCP tool provides structured parameters that map cleanly to the resume operation.

### Checkpoint State Extension

No changes needed to `CheckpointState`. The pending tool calls are already stored in the last assistant message's `tool_calls` field. When resuming:

1. Load checkpoint messages
2. Find the last assistant message
3. If it has `tool_calls` without corresponding `role: tool` results → those are pending
4. Apply the resume decisions to those pending tool calls

**Reasoning**: The existing checkpoint format already captures everything needed. We just need to interpret it correctly on resume.

### AutoApproveConfig in Async Mode

The existing `AutoApproveConfig` (used by TUI's `AutoApproveManager`) will be respected in async mode:

- `AutoApprove` tools: Execute immediately (current behavior)
- `Prompt` tools: Trigger pause
- `Never` tools: Trigger pause (or reject immediately, configurable)

**Reasoning**: Same config file works in both modes. Users don't need to maintain separate configurations.

## Integration Points

### CI/CD (GitHub Actions Example)

```yaml
- name: Run agent
  id: agent
  run: stakpak -a "deploy to staging" --output json > result.json
  continue-on-error: true

- name: Handle pause
  if: ${{ steps.agent.outputs.exit_code == 10 }}
  run: |
    CHECKPOINT=$(jq -r .checkpoint_id result.json)
    TOOLS=$(jq -r '.pause_reason.pending_tool_calls[].id' result.json)
    echo "Agent paused. Pending tools: $TOOLS"
    echo "checkpoint=$CHECKPOINT" >> $GITHUB_OUTPUT
    # Could: post to Slack, create issue, wait for webhook

- name: Resume after approval
  if: ${{ steps.approval.outputs.approved == 'true' }}
  run: stakpak -c ${{ steps.handle-pause.outputs.checkpoint }} --approve-all
```

### Parent Agent (Subagent Supervision)

```
Parent agent:
1. subagent_task("deploy to prod") → task_id: abc123
2. wait_for_tasks("abc123") → status: Paused
3. get_task_details("abc123") → sees pending terraform apply
4. (decides based on context)
5. resume_task("abc123", tool_decisions: [{tc_1, approved: true}])
6. wait_for_tasks("abc123") → status: Completed
```

### Human via CLI

```bash
$ stakpak -a "deploy" --output json
{
  "outcome": "paused",
  "checkpoint_id": "abc-123",
  "pause_reason": {"type": "tool_approval_required", ...},
  "resume_hint": "stakpak -c abc-123 --approve tc_1"
}

$ stakpak -c abc-123 --approve tc_1
# Agent resumes, executes terraform apply, completes
```

### TUI (Interactive Mode)

No changes needed. The TUI already has the approval bar for tool decisions and text input for prompts. The pause/resume mechanism is for headless mode only.

## Alternatives Considered

### Alternative 1: Long-Running Process with Stdin

The agent blocks reading from stdin when it needs input.

**Rejected because**:
- Blocks a process indefinitely (resource waste)
- stdin may not be available in all CI environments
- Doesn't work well with process supervisors that expect processes to exit
- No clean way to timeout or cancel

### Alternative 2: Webhook/Polling Server

The agent starts an HTTP server or polls an endpoint for approval.

**Rejected because**:
- Requires network infrastructure
- Firewall/networking complexity in CI environments
- Long-running process problem remains
- Overkill for the use case

### Alternative 3: Separate "Agent Task" Abstraction

Keep task manager process-centric, add a new "AgentTask" layer that tracks logical tasks across process invocations.

**Rejected because**:
- Two layers of abstraction adds complexity
- Task manager already has the right primitives (start, cancel, status)
- Adding `Paused` status and `resume` operation is simpler

### Alternative 4: `request_human_input` Tool

Add a special tool the LLM can call to explicitly request input.

**Rejected because**:
- Unnecessary — LLM already expresses "I need input" by responding with text and no tool calls
- Adds complexity to the tool set
- The natural pause point already exists

## Migration and Compatibility

### Backward Compatibility

- **Async mode default behavior unchanged**: Auto-execute all tools (current behavior)
- **Pause only activates with explicit flag**: New `--pause-on-approval` flag required to enable pause behavior
- Existing checkpoints work (no schema changes)
- Task manager's existing statuses and operations unchanged
- Existing scripts and CI/CD pipelines continue to work without modification

### New Flag

```bash
# Current behavior (unchanged) - auto-execute all tools
stakpak -a "deploy to staging"

# New behavior - pause when tools require approval
stakpak -a --pause-on-approval "deploy to staging"
```

When `--pause-on-approval` is set:
- `AutoApproveConfig` is respected
- Tools flagged as `Prompt` or `Never` trigger a pause
- Text-only responses (no tool calls) trigger a pause

When `--pause-on-approval` is NOT set (default):
- All tools auto-execute (current behavior)
- Text-only responses treated as completion (current behavior)

### Recommendations

- `--output json` recommended for machine consumers (CI/CD, parent agents)
- Pause manifest file always written on exit 10 (no flag needed)

## Security Considerations

### Checkpoint Access

Checkpoints contain full conversation history including potentially sensitive data. Access control for checkpoints should match access control for the agent itself.

### Tool Approval Bypass

A malicious actor with access to resume could approve dangerous tools. Mitigation:
- Resume requires the checkpoint_id (not guessable)
- Audit logging of resume operations
- Optional: require additional authentication for resume

### Pause Manifest Exposure

The pause manifest contains tool call details which may include sensitive arguments. The manifest is written to `.stakpak/session/` which should be in `.gitignore`.

## Design Decisions (Resolved)

1. **Default behavior for unspecified tools**: Unspecified tools are **rejected**. If resuming with `--approve tc_1` but there are also `tc_2` and `tc_3` pending, those unspecified tools receive `TOOL_CALL_REJECTED` as their result.

2. **Pause timeout**: **No timeout needed**. The process exits on pause (exit code 10), so there's no long-running process to timeout. Orchestrators can implement their own staleness checks on paused tasks if needed.

3. **Multiple pause cycles**: **No limits**. An agent can pause and resume any number of times. This supports complex workflows where multiple approval gates or clarification rounds are needed.

4. **Partial tool execution**: If 3 tools are approved and the first one fails, the agent **continues** with the remaining tools (matches TUI behavior).

## Implementation Plan

### Phase 1: Core Pause Mechanism
- Add `AutoApproveManager` to async mode
- Implement pause detection (tools requiring approval, or no tool calls)
- Add exit code 10 handling
- Write pause manifest on pause
- Save checkpoint on pause (standard checkpoint, no schema changes)

### Phase 2: Resume CLI
- Add `--approve`, `--reject`, `--approve-all`, `--reject-all` flags
- Implement resume logic: detect pending tool calls from last assistant message
- Handle both tool decisions and text prompt resume

### Phase 3: Task Manager Integration
- Add `TaskStatus::Paused`
- Add `resume_task` operation
- Store pause info in Task struct
- Maintain task ID across resume cycles

### Phase 4: MCP Tool
- Add `resume_task` MCP tool
- Structured parameters for tool decisions
- Integration with task manager

### Phase 5: Documentation and Testing
- Update CLI help
- Add examples for CI/CD integration
- Integration tests for pause/resume cycles
- Test subagent pause/resume scenarios

## Files Changed

### CLI

**`cli/src/main.rs`**
- Add new CLI flags: `--pause-on-approval`, `--approve`, `--reject`, `--approve-all`, `--reject-all`
- Handle exit code 10 for paused outcome
- Pass flags to async mode

**`cli/src/commands/agent/run/mode_async.rs`**
- Import and instantiate `AutoApproveManager`
- Add pause detection before tool execution loop
- Handle resume with tool decisions (execute approved, reject others)
- Write pause manifest on exit 10
- Change return type to distinguish Completed/Paused/Failed

**`cli/src/commands/agent/run/mod.rs`**
- Export new types (`AsyncOutcome`, `PauseReason`) if needed
- Update `RunAsyncConfig` to include approval flags

### Task Manager

**`libs/shared/src/task_manager.rs`**
- Add `TaskStatus::Paused` variant
- Add `pause_info: Option<PauseInfo>` to `Task` struct
- Add `PauseInfo` struct (checkpoint_id, raw_output)
- Detect exit code 10 → set status to `Paused` instead of `Failed`
- Add `Resume` variant to `TaskMessage` enum
- Implement `resume_task` operation (spawn new process, reuse task ID)
- Add `resume_task` method to `TaskManagerHandle`

### MCP Server

**`libs/mcp/server/src/local_tools.rs`**
- Add `resume_task` tool with parameters: task_id, tool_decisions, prompt
- Add `ResumeTaskRequest` struct with schemars derive
- Add `ToolDecisionInput` struct

### Shared (if needed)

**`libs/shared/src/local_store.rs`**
- Add helper to write pause manifest (`pause.json`)

### TUI (move for reuse)

**`tui/src/services/auto_approve.rs`**
- May need to move `AutoApproveManager` to `libs/shared` so async mode can use it
- Or duplicate the logic in async mode (simpler, less coupling)

## Files Unchanged

- `cli/src/commands/agent/run/checkpoint.rs` — no schema changes needed
- `libs/api/src/storage.rs` — `CheckpointState` unchanged
- `tui/src/services/approval_bar.rs` — TUI-specific, not needed for async
- `tui/src/event_loop.rs` — interactive mode unchanged

## New Files

**`cli/src/commands/agent/run/pause.rs`** (optional)
- Pause manifest writing
- Resume input parsing
- Helper functions for pause/resume logic

## References

- Existing checkpoint implementation: `cli/src/commands/agent/run/checkpoint.rs`
- Task manager: `libs/shared/src/task_manager.rs`
- Async mode: `cli/src/commands/agent/run/mode_async.rs`
- AutoApproveManager: `tui/src/services/auto_approve.rs`
- Approval bar: `tui/src/services/approval_bar.rs`
