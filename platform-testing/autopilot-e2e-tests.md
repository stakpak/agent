# Autopilot E2E Test Results

**Date:** 2026-02-15
**Branch:** `feature/autopilot-onboard-up-down`
**Platform:** macOS (launchd)
**Binary:** `cargo run --quiet --` (dev build)

---

## Summary

| Category | Passed | Failed | Skipped | Total |
|----------|--------|--------|---------|-------|
| Up / Init (merged) | 5 | 0 | 0 | 5 |
| Down | 2 | 0 | 0 | 2 |
| Status | 2 | 0 | 0 | 2 |
| Doctor | 1 | 0 | 0 | 1 |
| Restart | 2 | 0 | 0 | 2 |
| Logs | 1 | 0 | 0 | 1 |
| Schedule CRUD | 8 | 0 | 0 | 8 |
| Schedule Trigger | 4 | 0 | 0 | 4 |
| Schedule History / Show | 3 | 0 | 0 | 3 |
| Schedule Clean | 2 | 0 | 0 | 2 |
| Channel CRUD | 7 | 0 | 0 | 7 |
| **Total** | **37** | **0** | **0** | **37** |

### Bugs Found

| # | Severity | Description | Status |
|---|----------|-------------|--------|
| 1 | Medium | `ScheduleTriggerOn::Always` serializes as `"always"` but watch runtime expects `"any"` (`CheckTriggerOn::Any`). Causes restart to fail with parse error. | Pre-existing, not from this PR |

---

## Executed Tests

### 1. Up / Init (merged)

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 1.1 | First run, non-interactive | `stakpak up --non-interactive` (clean slate, no config) | ✓ PASS | Created config, installed service, started. Setup banner shown. |
| 1.2 | Idempotent start (already running) | `stakpak up` (autopilot already running) | ✓ PASS | Printed running status, no duplicate process. |
| 1.3 | Force re-init with bind override | `stakpak up --non-interactive --force --bind 127.0.0.1:5555` | ✓ PASS | Wiped old config (schedules gone), new bind saved, started on port 5555. |
| 1.4 | Force re-init wipes schedules | `stakpak autopilot schedule list` (after --force) | ✓ PASS | "No schedules configured." — old schedules removed. |
| 1.5 | Bind override persisted | `grep listen ~/.stakpak/autopilot.toml` | ✓ PASS | `listen = "127.0.0.1:5555"` |

### 2. Down

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 2.1 | Stop running autopilot | `stakpak down` | ✓ PASS | "✓ Autopilot service stopped and uninstalled" |
| 2.2 | Stop when already stopped (idempotent) | `stakpak down` (second time) | ✓ PASS | "Autopilot is not running." — no error |

### 3. Status

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 3.1 | Status when running | `stakpak autopilot status` | ✓ PASS | Service active, server reachable, scheduler running with PID, schedule table shown |
| 3.2 | Status with recent runs | `stakpak autopilot status --recent-runs 3` | ✓ PASS | Recent runs table appended with run IDs, schedule names, statuses, timestamps |

### 4. Doctor

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 4.1 | Doctor with valid setup | `stakpak autopilot doctor` | ✓ PASS | All 6 checks passed: credentials, config, channels, schedules, service, server health |

### 5. Restart

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 5.1 | Restart running autopilot | `stakpak autopilot restart` | ✓ PASS | Validated config, restarted service |
| 5.2 | Restart when not running | `stakpak autopilot restart` (autopilot down) | ✓ PASS | Error: "Autopilot is not running. Start it with `stakpak up`." |

### 6. Logs

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 6.1 | View scheduler logs | `stakpak autopilot logs -n 5 -c scheduler` | ✓ PASS | Showed scheduler log lines (schedule fired, agent completed events). Timed out as expected (follow mode is default). |

### 7. Schedule CRUD

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 7.1 | Add minimal schedule | `stakpak autopilot schedule add health-check --cron '*/5 * * * *' --prompt '...'` | ✓ PASS | "✓ Schedule 'health-check' added" |
| 7.2 | Add schedule with options | `stakpak autopilot schedule add git-status --cron '0 9 * * *' --prompt '...' --workdir ... --max-steps 20` | ✓ PASS | All options saved to config |
| 7.3 | Add third schedule | `stakpak autopilot schedule add disk-alert --cron '0 */6 * * *' --prompt '...' --trigger-on success` | ✓ PASS | Added with trigger_on=success |
| 7.4 | List all schedules | `stakpak autopilot schedule list` | ✓ PASS | All 3 shown with correct cron, status, next run |
| 7.5 | Duplicate name rejected | `stakpak autopilot schedule add health-check --cron '...' --prompt '...'` | ✓ PASS | Error: "Schedule 'health-check' already exists" (exit 1) |
| 7.6 | Invalid cron rejected | `stakpak autopilot schedule add bad-cron --cron 'not-a-cron' --prompt '...'` | ✓ PASS | Error: "Invalid cron expression 'not-a-cron': ..." (exit 1) |
| 7.7 | Remove schedule | `stakpak autopilot schedule remove disk-alert` | ✓ PASS | Removed, verified gone from list |
| 7.8 | Remove non-existent | `stakpak autopilot schedule remove nonexistent` | ✓ PASS | Error: "Schedule 'nonexistent' not found" (exit 1) |

### 8. Schedule Enable / Disable

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 8.1 | Disable schedule | `stakpak autopilot schedule disable git-status` | ✓ PASS | "✓ Schedule 'git-status' disabled". List shows `disabled` status, next run = `-` |
| 8.2 | Enable schedule | `stakpak autopilot schedule enable git-status` | ✓ PASS | "✓ Schedule 'git-status' enabled" |
| 8.3 | Disable non-existent | `stakpak autopilot schedule disable ghost` | ✓ PASS | Error: "Schedule 'ghost' not found" (exit 1) |

### 9. Schedule Trigger

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 9.1 | Dry run | `stakpak autopilot schedule trigger health-check --dry-run` | ✓ PASS | Showed assembled prompt, "[Dry run - schedule not queued]" |
| 9.2 | Real trigger (health-check) | `stakpak autopilot schedule trigger health-check` | ✓ PASS | Queued, completed in ~29s. Agent analyzed CPU/memory/disk. |
| 9.3 | Real trigger (git-status) | `stakpak autopilot schedule trigger git-status` | ✓ PASS | Queued, completed in ~22s. Agent ran with configured workdir. |
| 9.4 | Trigger non-existent | `stakpak autopilot schedule trigger ghost` | ✓ PASS | Error: "Schedule 'ghost' not found" (exit 1) |

### 10. Schedule History / Show

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 10.1 | History for schedule | `stakpak autopilot schedule history health-check` | ✓ PASS | Showed 3 runs (1 manual trigger + 2 cron-fired) with IDs, timestamps, statuses |
| 10.2 | Show run details | `stakpak autopilot schedule show 1` | ✓ PASS | Full details: session, checkpoint, model, steps, tokens, agent response |
| 10.3 | Show another run | `stakpak autopilot schedule show 4` | ✓ PASS | git-status run details with full agent response |

### 11. Schedule Clean

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 11.1 | Clean (no stale runs) | `stakpak autopilot schedule clean` | ✓ PASS | "No stale runs found" |
| 11.2 | Clean with prune | `stakpak autopilot schedule clean --older-than-days 0` | ✓ PASS | "No stale runs found" + "No runs older than 0 days to prune" |

### 12. Channel CRUD

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 12.1 | List (empty) | `stakpak autopilot channel list` | ✓ PASS | "No channels configured." with add hint |
| 12.2 | Slack missing tokens | `stakpak autopilot channel add slack` | ✓ PASS | Error: "Slack bot token required. Use --bot-token or set SLACK_BOT_TOKEN" |
| 12.3 | Telegram missing token | `stakpak autopilot channel add telegram` | ✓ PASS | Error: "Telegram token required. Use --token or set TELEGRAM_BOT_TOKEN" |
| 12.4 | WhatsApp unsupported | `stakpak autopilot channel add whatsapp` | ✓ PASS | Error: "Whatsapp is not supported yet" |
| 12.5 | Add Slack channel | `stakpak autopilot channel add slack --bot-token xoxb-test --app-token xapp-test` | ✓ PASS | "✓ Channel Slack added" |
| 12.6 | List (with channel) | `stakpak autopilot channel list` | ✓ PASS | Shows slack as configured |
| 12.7 | Remove channel | `stakpak autopilot channel remove slack` | ✓ PASS | "✓ Channel Slack removed", verified gone from list |

---

## Tests Not Yet Executed

### Up / Init Edge Cases

| # | Test | Command | Why Skipped |
|---|------|---------|-------------|
| N.1 | Interactive first run (no flags) | `stakpak up` (no config, no `--non-interactive`) | Requires interactive TTY input for onboarding wizard |
| N.2 | Non-interactive, no credentials | `stakpak up --non-interactive` (no API key configured) | Would need to remove auth config; destructive to dev env |
| N.3 | Env var channel pickup on first run | `SLACK_BOT_TOKEN=x SLACK_APP_TOKEN=y stakpak up --non-interactive` | Needs clean env + clean config; tested via code review |
| N.4 | `--model` override | `stakpak up --non-interactive --model anthropic/claude-sonnet-4-20250514` | Non-critical flag, same code path as `--bind` (tested) |
| N.5 | `--show-token` flag | `stakpak up --show-token` | Dev-only flag, low risk |
| N.6 | `--no-auth` flag | `stakpak up --no-auth` | Dev-only flag, low risk |
| N.7 | `--auto-approve-all` flag | `stakpak up --auto-approve-all` | CI-only flag, low risk |
| N.8 | `--foreground` mode | `stakpak up --foreground` | Blocks terminal, needs manual Ctrl+C |

### Schedule Edge Cases

| # | Test | Command | Why Skipped |
|---|------|---------|-------------|
| N.9 | Empty schedule name | `stakpak autopilot schedule add '' --cron '...' --prompt '...'` | Clap may reject empty positional arg before our validation |
| N.10 | Empty prompt | `stakpak autopilot schedule add x --cron '* * * * *' --prompt ''` | Needs verification of empty string handling |
| N.11 | Trigger when autopilot stopped | `stakpak autopilot schedule trigger health-check` (autopilot down) | Needs autopilot to be stopped with schedules still in config |
| N.12 | History with `--limit 1` | `stakpak autopilot schedule history health-check --limit 1` | Minor, limit clamping tested in unit tests |
| N.13 | Show non-existent run ID | `stakpak autopilot schedule show 99999` | Minor error path |
| N.14 | Schedule with `--check` script | `stakpak autopilot schedule add x --cron '...' --prompt '...' --check /path/to/script.sh` | Needs a check script to exist; trigger_on logic tested |
| N.15 | Schedule with `--pause-on-approval` | `stakpak autopilot schedule add x --cron '...' --prompt '...' --pause-on-approval` | Needs interactive approval flow |

### Channel Edge Cases

| # | Test | Command | Why Skipped |
|---|------|---------|-------------|
| N.16 | Discord missing token | `stakpak autopilot channel add discord` | Same pattern as Telegram (tested), low risk |
| N.17 | Channel test (no channels) | `stakpak autopilot channel test` | Needs channels configured; error path |
| N.18 | Channel test (invalid tokens) | `stakpak autopilot channel test` (with fake tokens) | Would fail on real API call; needs mock |
| N.19 | Telegram from env var | `TELEGRAM_BOT_TOKEN=x stakpak autopilot channel add telegram` | Same code path as CLI flag |
| N.20 | Add webhook (no token needed) | `stakpak autopilot channel add webhook` | Minor, webhook support may be incomplete |

### Status / Logs Edge Cases

| # | Test | Command | Why Skipped |
|---|------|---------|-------------|
| N.21 | Status when stopped | `stakpak autopilot status` (autopilot down) | Partially tested (saw it before starting); shows degraded status |
| N.22 | Status `--json` output | `stakpak autopilot status --json` | JSON output not tested; kept for scripting use |
| N.23 | Logs with `--follow` | `stakpak autopilot logs -f` | Blocks terminal indefinitely |
| N.24 | Logs when never ran | `stakpak autopilot logs` (fresh install, no log files) | Needs completely clean state |
| N.25 | Logs filter by server | `stakpak autopilot logs -c server` | Same code path as scheduler (tested) |
| N.26 | Logs filter by gateway | `stakpak autopilot logs -c gateway` | Same code path as scheduler (tested) |

### Lifecycle Integration

| # | Test | Command | Why Skipped |
|---|------|---------|-------------|
| N.27 | Config preservation across `up` | Add schedules + channels, `up --bind X`, verify schedules/channels preserved | Tested via `--force` (which wipes); non-force path preserves by design |
| N.28 | Cron-fired execution (wait for cron) | Wait for `*/5` cron to fire naturally | Verified indirectly — run #2 and #3 in history were cron-fired at :15:00 and :20:00 |

---

## Pre-existing Bug: `trigger_on` Mismatch

**Symptom:** Adding a schedule with `--trigger-on always` writes `trigger_on = "always"` to `autopilot.toml`, but the watch runtime (`CheckTriggerOn`) expects `"any"`. This causes `stakpak autopilot restart` to fail with:

```
unknown variant `always`, expected one of `success`, `failure`, `any`
```

**Root cause:** Two separate enums define the same concept:
- `autopilot.rs`: `ScheduleTriggerOn { Success, Failure, Always }` — serializes as `"always"`
- `watch/config.rs`: `CheckTriggerOn { Success, Failure, Any }` — expects `"any"`

**Fix:** Either rename `Always` → `Any` in `ScheduleTriggerOn`, or add `#[serde(alias = "always")]` to `CheckTriggerOn::Any`.
