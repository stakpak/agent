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

## Autopilot deployment readiness

Autopilot now has a shared readiness/probe system used by both:

- `stakpak up` — fail-fast startup checks
- `stakpak autopilot doctor` — fuller deployment-readiness report

### What `stakpak up` checks before startup

Blocking failures:

- credentials configured
- Docker installed
- Docker accessible to the current user
- clearly unsafe memory conditions

Warnings:

- bind-port conflicts
- disabled systemd linger
- low memory headroom

### What `stakpak autopilot doctor` checks

In addition to the startup probes, doctor also reports:

- disk space headroom
- critical sandbox mount readability hints
- channel config validity
- schedule config validity
- service installation status
- server health reachability
- tool approval posture

### Important behavior notes

- `stakpak up` now runs preflight checks before image pull/service start
- sandbox permission issues are addressed by mapping the host UID/GID into the container runtime when possible
- secret/config files should **not** be made world-readable as a workaround

### Common probe meanings

| Probe | Meaning | Typical fix |
|------|---------|-------------|
| `docker_installed` | Docker binary missing | Install Docker |
| `docker_accessible` | User cannot talk to daemon | Add user to docker group / start daemon |
| `memory` | Host is too small or borderline | Use 2GB+ RAM or add swap |
| `disk_space` | Low free space for image pulls/logs | Free space or expand volume |
| `bind_port` | Listen address unavailable | `stakpak down` or change bind |
| `systemd_linger` | User service may stop after logout | `sudo loginctl enable-linger $USER` |
| `sandbox_mount_inputs` | Critical mounted inputs may be unreadable | Fix invoking-user readability; do not loosen secret perms globally |

Use `stakpak autopilot doctor` as the canonical deployment-readiness and remediation entrypoint.

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
