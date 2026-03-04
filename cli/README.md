# Stakpak CLI

## Unified Configuration (Profiles + Autopilot)

This guide explains the current configuration model.

### TL;DR

- `~/.stakpak/config.toml` is the **source of truth** for behavior profiles.
- `~/.stakpak/autopilot.toml` is for **runtime wiring** (schedules/channels/service settings).
- Schedules/channels should reference profiles using `profile = "name"`.
- Runtime fields transported per-run are:
  - `model`
  - `auto_approve`
  - `system_prompt`
  - `max_turns`
- Inline channel `model` / `auto_approve` still work for compatibility, but are deprecated.

---

## File ownership

### 1) `~/.stakpak/config.toml` (behavior profiles)

Use this for profile behavior and credentials.

```toml
[profiles.default]
api_key = "sk-..."
model = "anthropic/claude-sonnet-4-5"
allowed_tools = ["view", "search_docs", "run_command"]
auto_approve = ["view", "search_docs"]
system_prompt = "You are the production reliability assistant."
max_turns = 64

[profiles.monitoring]
model = "anthropic/claude-haiku-4-5"
allowed_tools = ["view", "search_docs"]
auto_approve = ["view", "search_docs"]
system_prompt = "Monitor and report only. Never make changes."
max_turns = 16

[profiles.ops]
model = "anthropic/claude-sonnet-4-5"
allowed_tools = ["view", "search_docs", "run_command", "create", "str_replace"]
auto_approve = ["view", "search_docs", "run_command"]
max_turns = 64
```

### 2) `~/.stakpak/autopilot.toml` (runtime wiring)

Use this for schedules/channels and runtime config.

```toml
[server]
listen = "127.0.0.1:4096"

[[schedules]]
name = "health-check"
cron = "*/5 * * * *"
prompt = "Check production health"
profile = "monitoring"

[channels.slack]
bot_token = "xoxb-..."
app_token = "xapp-..."
profile = "ops"
```

---

## CLI workflow

### Add schedules with profile

```bash
stakpak autopilot schedule add health-check \
  --cron '*/5 * * * *' \
  --prompt 'Check production health and report anomalies' \
  --profile monitoring
```

`check` script paths support `~`, which resolves against the HOME of the user running autopilot.
For systemd/launchd/container deployments, prefer absolute paths (for example, `/home/ec2-user/.stakpak/checks/endpoints.sh`).

### Add channels with profile

```bash
stakpak autopilot channel add slack \
  --bot-token "$SLACK_BOT_TOKEN" \
  --app-token "$SLACK_APP_TOKEN" \
  --profile ops
```

Both commands validate that profile names exist in `config.toml`.

---

## Runtime resolution path

1. Caller selects a profile (schedule/channel/API caller).
2. Profile is resolved from `config.toml`.
3. Runtime fields are converted to `RunOverrides`.
4. Server merges `RunOverrides` with `AppState` defaults to build per-run `RunConfig`.

This keeps server runtime stateless while allowing per-run behavior.

---

## Backward compatibility

- Channel inline overrides are still supported:
  - `channels.<type>.model`
  - `channels.<type>.auto_approve`
- If both `profile` and inline values are set, profile-based run overrides take precedence.
- Gateway emits deprecation warnings to help migration.

---

## Validation limits

Profile validation enforces:

- `max_turns` in `1..=256`
- `system_prompt` up to `32KB` (characters)

Invalid profile values fail at profile resolution time.

---

## Recommended migration

1. Move channel inline `model` and `auto_approve` into named profiles in `config.toml`.
2. Set `profile = "..."` on channels and schedules.
3. Use `stakpak autopilot doctor` to detect deprecated inline channel fields.
4. Keep `autopilot.toml` focused on runtime wiring only.
