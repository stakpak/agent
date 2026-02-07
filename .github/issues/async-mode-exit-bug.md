# Async mode fails to exit gracefully and doesn't report session ID/stats

## Problem

When running `stakpak --async` (especially inside Docker containers for harbor eval), the agent:

1. **Doesn't exit when background tasks (child processes ) complete** - uses `std::process::exit(0)` as a workaround
2. **Doesn't fetch/display session stats** - unlike interactive mode which calls `get_session_stats()`
3. **Doesn't print session URL** - missing `https://stakpak.dev/{user}/agent-sessions/{id}`

## Reproduction

```dockerfile
# Example: Running async mode in Docker
FROM stakpak/agent:latest
RUN stakpak --async --prompt "Run background task"
# Agent may hang or force-exit without reporting stats
```

## Current vs Expected

| Feature | Interactive Mode | Async Mode (Bug) |
|---------|-----------------|------------------|
| Session Stats | ✅ Fetched | ❌ Missing |
| Session URL | ✅ Printed | ❌ Missing |
| Graceful Exit | ✅ Clean | ❌ `process::exit(0)` |

## Key Files

- `mode_async.rs:437-472` - Only prints session ID, then force-exits
- `mode_interactive.rs:1149-1192` - Properly fetches stats and prints URL

## Fix

Add to `mode_async.rs` before shutdown:
```rust
if let Some(session_id) = current_session_id {
    if let Ok(stats) = client.get_session_stats(session_id).await {
        print!("{}", renderer.render_session_stats(&stats));
    }
    if let Ok(account) = client.get_my_account().await {
        println!("https://stakpak.dev/{}/agent-sessions/{}", account.username, session_id);
    }
}
```

**Labels:** `bug`, `async-mode`
