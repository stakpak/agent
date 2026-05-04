---
description: "Rollback agent changes: /rollback [all|files|infra|<filter>]"
---

You are undoing changes you made earlier in this session — files you edited and cloud/infra you provisioned. Do it in four phases: discover, ask, confirm, execute.

---

Input: {input}

---

## Interpreting the Input

Treat `{input}` as a filter:

- **empty** — discover everything, then ask.
- **`all`** — discover everything, skip the selection prompt, still run the destructive-action confirmation.
- **`files`** — files only.
- **`infra`** — cloud/infra only.
- **anything else** — case-insensitive substring filter applied during discovery (e.g. `ec2`, `src/`, `my-bucket`).

Use best judgement on ambiguous input.

---

## Phase 1 — Discover

Build one flat list of rollback candidates. Each candidate has:

- `id` — stable value you will pass to `ask_user` (e.g. `file:/abs/path` or `aws:ec2-instance:i-0abc123:us-east-1`)
- `label` — one line describing what the reversal will do
- `reverse` — the exact command or tool call you will run
- `notes` — anything the user should know (e.g. "bucket non-empty", "no ID in transcript")

### 1.1 Files (skip if `{input}` is `infra`)

Every `create`, `str_replace`, and `remove` you ran in this session returned a `<file_backups>` XML block pairing `original_path` with `backup_path`. Walk back through your session output and collect those pairs.

Example of what you saw when you removed a file:

```
<file_backups>
    <file
        original_path="/Users/alice/project/src/app.py"
        backup_path=".stakpak/session/backups/525963a3.../app.py"
        location="local"
    />
</file_backups>
```

From that entry, emit a candidate:

- `id: file:/Users/alice/project/src/app.py`
- `label: Restore src/app.py from .stakpak/session/backups/525963a3.../app.py`
- `reverse: copy the backup back to the original path` (use the `create` tool if the original is gone, `str_replace` otherwise — whichever applies)

For files you **created** this session, the reversal is `remove` on the original path. The `remove` tool backs the file up again, so the undo is itself reversible.

If you cannot find a `<file_backups>` entry for a file you remember changing, emit the candidate with an empty `reverse` and `notes: "no backup entry in this session — manual review"`.

### 1.2 Cloud / infra (skip if `{input}` is `files`)

There is no ledger — reconstruct from the session transcript. Scan the commands you ran for mutating patterns and extract:

- **Resource type** (e.g. `ec2-instance`, `s3-bucket`, `iam-role`, `k8s-deployment`, `helm-release`)
- **Identifier** — prefer IDs the command actually printed to stdout. If no ID was ever printed, emit the candidate with empty `reverse` and `notes: "identifier not in transcript — manual review"`.
- **Region / account / namespace** where relevant
- **Original command** for display at confirmation time

Use the mapping table below to fill `reverse`.

### 1.3 Everything else (always emit as manual-review)

Flag, don't auto-reverse: package installs, `systemctl enable/start`, cron entries, DNS records, git commits/pushes, webhook calls, email sends. Each gets `reverse: ""` and `notes: "manual review — v1 does not auto-reverse this"`.

### 1.4 Apply the filter

If `{input}` is free text (not a reserved keyword), keep only candidates whose `id`, `label`, or originating command matches the filter (case-insensitive).

### 1.5 Empty list

If nothing matches, say "Nothing to rollback" and stop. Do not advance to Phase 2.

---

## Phase 2 — Ask

Skip this phase when `{input}` is `all`. Otherwise call `ask_user` once:

- `label: "Rollback"`
- `question: "Select the changes you want to rollback. Nothing will execute until you confirm in the next step."`
- `multi_select: true`
- `allow_custom: false`
- One option per candidate: `value` = candidate `id`, `label` = candidate `label`. Prefix manual-review candidates with `[MANUAL REVIEW] `.
- Final option: `value: "__cancel__"`, `label: "Cancel — do nothing"`.

If the user picks `__cancel__` or nothing, say "Cancelled — no changes made" and stop.

Selected candidates with an empty `reverse` go straight to the final report as `skipped (manual review)`.

---

## Phase 3 — Confirm

For the remaining candidates, call `ask_user` once more with a single-select question whose `question` body lists the plan line-by-line:

Example body:

```
About to run:
  1. Restore src/app.py from .stakpak/session/backups/525963a3.../app.py
  2. aws ec2 terminate-instances --instance-ids i-0abc123 --region us-east-1
  3. kubectl delete -f deploy.yaml
```

Options:

- `value: "proceed"`, `label: "Proceed — execute all the above"`
- `value: "abort"`, `label: "Abort — do nothing"`

If the user picks anything other than `proceed`, stop and say "Cancelled at confirmation."

**Credentials preflight** — before executing any cloud reversal, verify credentials for each target:

- AWS → `aws sts get-caller-identity`
- GCP → `gcloud auth list`
- Azure → `az account show`
- Kubernetes → `kubectl cluster-info`

If a target's credentials are missing or expired, mark every candidate for that target as `skipped (missing credentials)` and continue with the rest.

**Dry-run where supported** — for cloud deletes that accept `--dry-run`, run the dry-run first. If it succeeds, run the real command. If it fails, mark the candidate `failed (dry-run rejected)`.

---

## Phase 4 — Execute & Report

Run the remaining candidates one at a time. For each, record `ok`, `failed`, or `skipped` with a one-line detail.

**Keep going on failures.** One bad reversal never aborts the batch.

At the end, print a summary table:

| # | Item | Result | Detail |
|---|------|--------|--------|
| 1 | src/app.py | ✓ ok | restored from backup |
| 2 | i-0abc123 | ✗ failed | DependencyViolation: attached to ENI eni-xyz |
| 3 | deploy.yaml | – skipped | manual review |

Below the table, list each `failed` item with the full error and the exact manual command the user can run to finish the rollback themselves.

---

## Cloud / Infra Reversal Mapping

Use this to populate `reverse` for Phase 1.2.

| Forward action | Reverse action |
|----------------|----------------|
| `aws ec2 run-instances` / `ec2 create-instances` | `aws ec2 terminate-instances --instance-ids <id> --region <region>` |
| `aws s3 mb` / `s3api create-bucket` | `aws s3 rb s3://<bucket> --region <region>` (add `--force` only if the user approves force in Phase 3) |
| `aws iam create-<x>` | matching `aws iam delete-<x>` (detach policies / remove from groups first if needed) |
| `aws <service> create-<x>` / `put-<x>` | matching `delete-<x>` for that service |
| `terraform apply` | `terraform destroy -target=<addr>` per resource if the user picked specific items; full `terraform destroy` only if the user picked "all" |
| `kubectl apply -f <x>` / `kubectl create <x>` | `kubectl delete -f <x>` (or `kubectl delete <kind> <name> -n <ns>`) |
| `helm install <release>` | `helm uninstall <release> -n <ns>` |
| `gcloud <service> <resource> create` | `gcloud <service> <resource> delete` with the same identifier |
| `az <service> <resource> create` | `az <service> <resource> delete` with the same identifier |
| `docker run` (created a container) | `docker rm -f <container>` |
| `docker compose up` | `docker compose down` in the same project directory |
| Package install, `systemctl enable`, cron, DNS, webhook, email, git commit/push | manual review — do not auto-reverse |

---

## Rules

These are how you stay safe. Each one is framed as "do this, because..."

1. **Only act on candidates from Phase 1.** If it wasn't discovered, it isn't eligible. Example: the user types `/rollback ec2` and the filter produces zero candidates → say "Nothing matched" and stop. Do not go hunting for other EC2 instances.

2. **Only act on candidates the user selected in Phase 2.** Example: Phase 1 found 5 candidates, user ticked 2 → execute exactly those 2.

3. **Use IDs the session transcript printed.** Example: the user ran `aws ec2 run-instances ...` and the output contained `"InstanceId": "i-0abc123"` → use `i-0abc123`. If the output was truncated or never printed an ID → emit `manual review` with the original command so the user can reverse it themselves.

4. **Confirm before every destructive execution, even for `/rollback all`.** The only way to reach Phase 4 is a `proceed` answer in Phase 3.

5. **Verify credentials before any cloud call.** Example: before running `aws ec2 terminate-instances`, run `aws sts get-caller-identity`. If it fails, mark every AWS candidate as `skipped (missing credentials)` and continue with the rest of the batch.

6. **Prefer `--dry-run` where the tool supports it.** Example: run `aws ec2 terminate-instances --instance-ids i-0abc123 --dry-run` first; only run the real call if the dry-run returns the expected `DryRunOperation` response.

7. **One failure, keep going.** Example: terminating i-0abc123 fails with `DependencyViolation` → record the failure, move on to the next candidate, include the error and the unblocking command in the final report.

8. **Stay in this session.** The candidate list is built from this session's tool output. Example: the user has an EC2 instance `i-0oldprod` that predates this session → it will not appear in Phase 1 and must not be touched.

9. **Report everything.** Every selected candidate ends up in the summary table as `ok`, `failed`, or `skipped`. No silent drops.
