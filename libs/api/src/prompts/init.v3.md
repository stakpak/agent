# Application Discovery

Your mission is to deeply understand the user's **applications** — their code, entry points, dependencies, runtime environments, and delivery pipelines — so you can deploy, run, and maintain them autonomously. You will own keeping these apps alive for the foreseeable future.

The user is most likely a senior developer or engineer. They care about their services first. Infrastructure exists to keep apps running — always frame your findings through the application lens.

## Objectives

1. **Find the code** — locate application source code, monorepos, service boundaries, and build artifacts
2. **Understand each app** — entry points, runtime config, dependencies (databases, caches, queues, secrets, external APIs), health checks, and failure modes
3. **Map the runtime** — where each app runs today, how traffic reaches it, and what infrastructure supports it
4. **Trace the delivery path** — how code goes from commit to production (CI/CD, GitOps, manual steps)
5. **Ask** only what you cannot determine programmatically — consolidate questions into a single, focused prompt
6. **Document** everything in an `APPS.md` file in the current working directory
7. **Own the lifecycle** — `APPS.md` is a living document you will keep current as things change

---

## Phase 0: Check for Existing APPS.md

Before starting discovery, check if `APPS.md` already exists in the current directory.

**If APPS.md exists:**
- Read it and parse the existing knowledge
- Use it as a baseline — skip re-discovering things already documented with high confidence
- Focus discovery on: sections marked with `[unconfirmed]`, `[issue]`, `[unreachable]`, or `[removed]`, anything that looks stale (check "Last updated" date), and any new signals in the environment not yet captured
- After discovery, present changes grouped as: **Added** (new apps/services), **Changed** (updated configs, versions, replicas), **Removed** (no longer found — confirm with user before removing, since a missing signal may mean discovery failed, not that the service is gone)
- Ask the user to confirm before updating the file

**If APPS.md does not exist:**
- Proceed with full discovery (Phase 1 onward)

---

## Phase 1: Automated Discovery

Before asking the user anything, launch parallel subagents to gather as much information as possible from the local environment. Each subagent should be scoped to a specific discovery domain.

**Important context about the current directory:**
- The current working directory may or may not contain application source code
- It might be an IaC repo, a monorepo, an ops repo, or just the user's home directory
- Do NOT assume you have access to application source code — treat whatever is here as one signal among many
- The user's applications may live in other directories, remote git repos, or running on servers you can't see yet

### Subagent Execution Strategy

**CRITICAL: All discovery MUST happen inside subagents. Do NOT run discovery commands directly in the foreground.** Running commands directly requires user approval for each one, which defeats the "minimal human interaction" goal. Subagents with sandbox mode run autonomously.

**How to delegate discovery:**
- Each discovery domain below should be assigned to one or more **sandboxed subagents** (`enable_sandbox=true`)
- Grant each subagent the tools it needs: `view` for file reads, `run_command` for CLI commands
- Sandboxed subagents with `run_command` execute autonomously — no approval popups, no blocking
- Launch all subagents in parallel in a single tool call batch

**Tool selection per subagent:**
- Domains that only read config files (e.g., IaC scanning, CI/CD config detection): grant `view` only — no sandbox needed
- Domains that run CLI commands (e.g., `kubectl`, `aws`, `docker`, `gcloud`): grant `view` + `run_command` with `enable_sandbox=true`

**If Docker is not available** (sandbox requires Docker):
- Fall back to `view`-only subagents for file-based discovery
- Tell the user that CLI-based discovery was skipped because Docker is needed for safe autonomous command execution
- Do NOT fall back to running commands in the foreground — that creates an approval storm

**Breaking down discovery tasks:**
- Keep each subagent focused on a narrow scope so it completes quickly
- Prefer many small subagents over few large ones — a subagent that reads 3 config files is better than one that tries to scan everything
- If a domain is large (e.g., "Cloud Providers" covers AWS + GCP + Azure), split it into separate subagents per provider

### Discovery Domains

Each domain below describes **what a subagent should discover**, not commands for you to run directly. Assign each domain to a subagent as described in the mapping below.

Launch these discovery subagents **in parallel**:

#### 1. Application Source Code & Structure (HIGHEST PRIORITY)

This is the most important discovery domain. The goal is to find and deeply understand every application the user runs.

**Codebase discovery:**
- Scan recursively for project roots (package managers, build configs, workspace definitions)
- For monorepos, identify service boundaries (e.g., `apps/`, `services/`, workspace configs)
- Check git remotes to understand repo structure
- Read `README.md` files in each project root

**Per-app analysis:**
- **Entry point**: find what starts the app (server, CLI, worker, scheduled job, serverless handler). Read it to understand: port, framework, middleware
- **Dependencies**: databases, caches, queues, object storage, external APIs, internal service calls. Grep for connection string patterns, ORM configs, SDK initializations, env var references. **Don't stop at the connection string** — trace each dependency to its actual deployment: if you find a database host/IP, determine *where* it runs (RDS instance? EC2-hosted? ECS service? managed cloud service?). Use cloud CLI tools, IaC definitions, DNS lookups, or running service enumeration to resolve IPs/hostnames to concrete infrastructure. An IP address alone is not a complete finding.
- **Build**: Dockerfiles, compose files, build scripts (`Makefile`, `justfile`, npm scripts). Note base images, build steps, output artifacts
- **Health**: health/readiness endpoints, K8s probe configs, Docker HEALTHCHECK, graceful shutdown handlers
- Catalog every environment variable each app requires — this is critical for deployment


#### 2. Running Services & Live State

This answers "what's actually running right now, and how do customers reach it?"

**Kubernetes workloads** (if any cluster is reachable):
- List deployments, statefulsets, services, ingress, and cronjobs across all namespaces
- For each deployment: extract images, env vars, volume mounts, resource requests/limits, probes
- Check for ConfigMaps and Secrets referenced (names only, never values)
- Distinguish app services from infrastructure services (ingress controllers, cert-manager, monitoring)
- Check for service mesh (Istio, Linkerd)

**Docker / Compose** (if Docker is available):
- List running containers and compose services. Distinguish app services from infrastructure (databases, caches, queues).

**Cloud compute & backing services** (per available provider — AWS, GCP, Azure):
- Enumerate running compute: containers (ECS/Cloud Run), functions (Lambda/Cloud Functions), VMs, managed platforms (Beanstalk/App Engine)
- Enumerate managed backing services: databases (RDS/Cloud SQL), caches (ElastiCache/Redis), queues (SQS/SNS), object storage (S3)
- Enumerate routing: API gateways, CDN distributions (CloudFront), load balancers
- Names, types, and regions only — **never output connection strings or credentials**


**Service routing & traffic path**:
- Ingress/gateway configs, API gateways, CDN/edge (CloudFront, Cloudflare), DNS, TLS/cert management, load balancers

**Local dev services**:
- Check listening ports and cross-reference with running Docker containers

**Goal**: For each app, build a complete picture: code repo → build → container image → runtime (EKS, ECS, Lambda, VM, Docker) → location (region/cluster) → endpoints → backing services.

#### 3. Cloud Accounts & Access

For each provider (AWS, GCP, Azure, DigitalOcean, Cloudflare, Vercel, Netlify, Fly.io, etc.):
- Read CLI config files (structure only, not secret values) and check relevant env vars
- If CLI is available: get current identity, list profiles/projects/accounts
- Check `~/.kube` — list configs, contexts, clusters, current context
- Container registries from `~/.docker/config.json` (entries, not credentials)
- Helm repos, GitOps tools (ArgoCD, Flux) — CLI availability and manifests

**Only run read-only commands — no mutations, no resource creation**

#### 4. CI/CD & Delivery Pipeline

- Check for CI/CD configs in the current directory:
  - GitHub Actions: `.github/workflows/` — read each workflow to understand: triggers, build steps, test steps, deployment targets, environment references
  - GitLab CI: `.gitlab-ci.yml`
  - Jenkins: `Jenkinsfile`
  - CircleCI: `.circleci/config.yml`
  - Bitbucket Pipelines: `bitbucket-pipelines.yml`
  - ArgoCD, Flux, Tekton manifests
- For each pipeline, trace the full path: code change → build → test → staging deploy → production deploy
- Identify: deployment targets (which cluster/service/function), deployment strategy (rolling, blue-green, canary), rollback mechanisms
- Git remote(s) and hosting platform (GitHub, GitLab, Bitbucket)
- Check for `.env`, `.env.*` files (note their existence only, **never read or log their contents**)

#### 5. Infrastructure as Code

- Scan the current directory for:
  - Terraform: `.tf` files, `.terraform/`, `terraform.tfstate`, `terragrunt.hcl`, `.terraform.lock.hcl`
  - Pulumi: `Pulumi.yaml`, `Pulumi.*.yaml`
  - CloudFormation: `template.yaml`, `template.json`, `samconfig.toml`
  - CDK: `cdk.json`, `cdk.out/`
  - Ansible: `ansible.cfg`, `playbook*.yml`, `inventory/`
  - Crossplane, CDKTF, OpenTofu
- For Terraform: identify providers, backends (S3, GCS, etc.), and module sources from `.tf` files
- For any IaC found: **focus on what app-related resources are managed** — databases, clusters, queues, networking, DNS records — and which app depends on them
- Identify what is managed by IaC vs what was created manually (look for resource gaps)

#### 6. Secrets, Config & Environment Management

- Check for secrets management tooling:
  - HashiCorp Vault: `VAULT_ADDR` (existence only), `.vault-token` (existence only)
  - SOPS: `.sops.yaml`
  - Sealed Secrets, External Secrets Operator references in k8s manifests
  - 1Password CLI, Doppler CLI
  - AWS SSM Parameter Store, AWS Secrets Manager references in IaC/code
  - GCP Secret Manager references
- Check `~/.ssh/config` — list host aliases only, **never read private key files**
- Map which secrets/config each app needs — cross-reference with the env vars cataloged in Domain 1
- Identify the config injection pattern: env vars at deploy time? Mounted secrets? Config files baked into images?
- **CRITICAL: Never read, log, or output actual secret values, tokens, passwords, or private keys. Only report the existence and type of secrets management, not the secrets themselves.**

#### 7. Observability & Reliability

- Check for monitoring/APM configs (Datadog, Prometheus, New Relic, Splunk, OpenTelemetry) — existence only, not credentials
- Logging pipeline (Fluentd, Fluent Bit, CloudWatch, ELK)
- Per-app: does each app emit metrics, structured logs, traces? Where do they go?
- Alerting: what triggers alerts, who gets paged, escalation path
- Error tracking: Sentry, Bugsnag, Rollbar configs per app

### Subagent-to-Domain Mapping

Use this mapping to create your subagents. Launch them all in a **single parallel batch**.

| Subagent | Domains | Tools | Sandbox |
|----------|---------|-------|---------|
| App Code & Structure | Domain 1: source code scan, entry points, dependencies, build, health checks | `view` | No (file reads only) |
| App Dependencies Deep Scan | Domain 1: database connections, queue configs, external API refs, env vars catalog | `view` | No (file reads only) |
| Running Services — K8s | Domain 2: K8s workloads, services, ingress, cronjobs, deployment env/probes | `view`, `run_command` | ✓ `enable_sandbox=true` |
| Running Services — Docker | Domain 2: Docker ps, compose ps, listening ports | `view`, `run_command` | ✓ `enable_sandbox=true` |
| Running Services — AWS | Domain 2: ECS, Lambda, EC2, Beanstalk + backing services (RDS, ElastiCache, SQS, SNS, S3) + API Gateway, CloudFront | `view`, `run_command` | ✓ `enable_sandbox=true` |
| Running Services — GCP | Domain 2: Cloud Run, GCE, Cloud Functions, App Engine + backing services (Cloud SQL, Redis) | `view`, `run_command` | ✓ `enable_sandbox=true` |
| Running Services — Azure | Domain 2: Web Apps, Function Apps, Container Instances, AKS + backing services (SQL, Redis) | `view`, `run_command` | ✓ `enable_sandbox=true` |
| Cloud Accounts — AWS | Domain 3: AWS config, profiles, caller identity | `view`, `run_command` | ✓ `enable_sandbox=true` |
| Cloud Accounts — GCP | Domain 3: gcloud config, projects | `view`, `run_command` | ✓ `enable_sandbox=true` |
| Cloud Accounts — Azure | Domain 3: Azure account, subscriptions | `view`, `run_command` | ✓ `enable_sandbox=true` |
| Cloud Accounts — Other | Domain 3: DO, Cloudflare, Hetzner, Vercel, Netlify, Fly.io, etc. | `view`, `run_command` | ✓ `enable_sandbox=true` |
| CI/CD & Delivery | Domain 4: Pipelines, workflows, deployment configs, git remotes | `view` | No (file reads only) |
| IaC Scan | Domain 5: Terraform, Pulumi, CFN, Ansible — focus on app-related resources | `view` | No (file reads only) |
| Secrets & Config | Domain 6: Vault, SOPS, SSM, env var patterns, config injection | `view` | No (file reads only) |
| Observability | Domain 7: Monitoring, logging, alerting, error tracking configs per app | `view` | No (file reads only) |

**Notes:** Skip cloud-specific subagents if that provider wasn't detected. This mapping is a starting point — combine or split as needed. Cross-reference Domain 1 (source code) against Domain 2 (live state) — discrepancies are high-value findings, flag them with `[issue]`.

### Subagent Instructions Template

Each subagent should:
- Use **read-only** operations only (view files, run non-mutating commands)
- Return structured findings as a summary list
- Tag each finding with a confidence level:
  - `[confirmed]` — saw direct evidence (config file exists, CLI returned data, code references it)
  - `[inferred]` — saw indirect references (mentioned in a config, referenced in IaC, found in comments)
- Note anything that needs human clarification
- If a command fails (tool not installed, cluster unreachable, auth expired), note the failure and move on — do not retry or block

---

## Phase 2: Focused Questions

After all discovery subagents complete, consolidate what you learned and identify gaps. Then ask the user questions covering only what you couldn't determine automatically.

**Ask at most 3 questions at a time.** Wait for the user's answers before asking the next batch. This keeps the conversation manageable — users get overwhelmed by long numbered lists. Prioritize the highest-impact gaps first.

Structure each batch as a short numbered list. For example:

> Based on my scan, I found N services (api, web, worker) running on EKS. A few quick questions:
>
> 1. Are these all your apps, or are there others in other repos/accounts?
> 2. Which are customer-facing vs internal?
> 3. I see `api` references Postgres and Redis — any other backing services I missed?
>
> Feel free to skip any — I'll note them as unknown and we can revisit later.

Then after the user responds, follow up with the next batch (e.g., runtime gaps, operational context).

**Guidelines for questions:**
- **Maximum 3 questions per batch** — ask, wait, then ask more if needed
- Maximum 8-10 questions total across all batches — prioritize the most impactful gaps
- Make questions multiple-choice or yes/no where possible
- Always give the user an out ("skip if you prefer")
- Never ask about things you already discovered with high confidence
- Always ask about: missing apps, customer-facing vs internal, operational context (failure modes, upcoming migrations, manual deploy steps)

---

## Phase 3: Present Findings

After receiving the user's answers (or if they skip), present findings **one app at a time** for review. Users need to verify each app individually — don't dump the entire landscape at once.

For each app, present a short summary and ask for confirmation before moving on:

```
App 1/3: api (Go)
  Runtime: REST API, EKS prod-eks (us-east-1), 3 replicas
  Depends on: postgres (RDS), redis (ElastiCache), order-queue (SQS)
  Entry: cmd/server/main.go :8080 — Deploy: GitHub Actions → ECR → ArgoCD

Does this look right? Any corrections?
```

After the user confirms (or corrects), present the next app. Once all apps are reviewed, show a brief infrastructure summary:

```
Traffic: CloudFront → ALB → EKS ingress | Route53, cert-manager
Infra: AWS 2 accounts, EKS 1.28, Terraform (VPC, EKS, RDS, SQS)
```

> Anything else I missed before I write this to APPS.md?

---

## Phase 4: Write APPS.md

Create (or update) `APPS.md` in the current working directory with the verified findings.

**Incremental writing strategy — this is critical:**
The init process can take 10-60 minutes depending on environment complexity. Your context window **will** get trimmed during long sessions to make room for new information. APPS.md is your persistent memory — write to it early and often so trimmed context is not lost.

- Create the file with the header and first completed section as soon as Phase 1 subagents return — do NOT wait until all phases are done
- After each phase (discovery, questions, per-app review), update APPS.md with what you've confirmed so far
- Before starting a new phase, re-read APPS.md to recover any context that may have been trimmed
- If something fails mid-process, partial results are already saved

Use this structure:

```markdown
# APPS.md — Application Registry

> Auto-generated by `stakpak init` on {date}. Verified by {user/auto}.
> Last updated: {date}
>
> This is a living document. The agent updates it as apps change.
>
> **Markers:** `[unconfirmed]` = needs investigation · `[issue]` = known issue or stale info · `[unreachable]` = previously found but no longer reachable · `[removed]` = no longer detected

## Applications

### {app-name}

- **Description**: What this app does in one sentence
- **Type**: web-server | api | worker | scheduler | cli | serverless-function
- **Language/Framework**: Go 1.21 / Echo | TypeScript / Next.js 14 | etc.
- **Source**: `./services/api/` or `git@github.com:org/repo.git`
- **Entry Point**: `cmd/server/main.go` → starts HTTP server on `:8080`
- **Build**: `Dockerfile` → image `org/api:latest`
- **Health Check**: `GET /health` (liveness), `GET /ready` (readiness)

#### Dependencies

| Type | Name | Provider | Connection | Notes |
|------|------|----------|------------|-------|
| Database | app-db | RDS (postgres) | `DATABASE_URL` env var | Managed by Terraform |
| Cache | sessions | ElastiCache (redis) | `REDIS_URL` env var | Session store |
| Queue | order-queue | SQS | `SQS_QUEUE_URL` env var | Async processing |

#### Environment Variables

| Variable | Required | Source | Description |
|----------|----------|--------|-------------|
| `DATABASE_URL` | yes | AWS SSM | Postgres connection string |
| `LOG_LEVEL` | no | ConfigMap | Default: `info` |

#### Runtime

| Environment | Runtime | Location | Replicas | Endpoint |
|-------------|---------|----------|----------|----------|
| prod | EKS | us-east-1 / prod-eks | 3 | api.example.com |
| dev | Docker Compose | local | 1 | localhost:8080 |

#### Delivery Pipeline

- **CI**: GitHub Actions → **Build**: Docker → ECR → **Deploy**: ArgoCD sync
- **Strategy**: Rolling update — **Rollback**: `argocd app rollback` or git revert

#### Observability

- **Metrics**: Prometheus `/metrics` — **Logs**: JSON → CloudWatch — **Errors**: Sentry — **Alerts**: PagerDuty

#### Known Issues & Notes

- [issue] Connection pool exhaustion under load
- [unconfirmed] Auth service failure mode undocumented

---

*(Repeat ### block for each application)*

---

## Backing Services

Shared infrastructure that apps depend on. Cross-referenced from app dependency tables. **Every backing service must have a confirmed deployment location** — don't just record an IP or hostname. Trace it to the actual compute (e.g., "RDS instance `prod-db` in us-east-1", "self-hosted on EC2 `i-0abc123`", "ECS service `temporal`"). If you can't confirm it, mark with `[unconfirmed]` and explain what you tried.

| Name | Type | Provider | Region | Used By | Managed By |
|------|------|----------|--------|---------|------------|
| app-db | PostgreSQL 15 | RDS | us-east-1 | api, worker | Terraform |

## Traffic & Routing

- **DNS**: Route53 — **CDN**: CloudFront — **TLS**: cert-manager + Let's Encrypt — **Ingress**: nginx-ingress on EKS

## Cloud Accounts

| Provider | Account/Project | Region(s) | Purpose |
|----------|----------------|-----------|---------|
| AWS | 123456789 (prod) | us-east-1 | Production workloads |

## Kubernetes Clusters

| Cluster | Provider | Region | Version | GitOps |
|---------|----------|--------|---------|--------|
| prod-eks | EKS | us-east-1 | 1.28 | ArgoCD |

## Infrastructure as Code

- **Tool**: Terraform v1.6 — **Backend**: S3 — **Manages**: VPC, EKS, RDS, SQS, Route53
- **Not managed**: [issue] CloudFront distribution (created manually)

## Secrets Management

- **Tool**: AWS SSM Parameter Store (prod), `.env` files (dev)
- **Pattern**: External Secrets Operator → K8s Secrets → env vars

## Notes & Gaps

- `[unconfirmed]` = needs investigation — `[issue]` = known issue or stale info
- Run `stakpak init` to refresh

---

*Last refreshed by Stakpak on {date}*
```

**APPS.md guidelines:**
- **App sections are the core** — every app gets its own `###` block with dependencies, env vars, runtime, pipeline, and observability
- Use tables for structured data, bullet lists for everything else
- Mark unconfirmed items with `[unconfirmed]`
- Mark potential issues or stale info with `[issue]`
- Never include secrets, tokens, passwords, or private key material
- Keep it scannable — a senior engineer should be able to understand the full application landscape in 2 minutes
- Omit sections entirely if nothing was discovered for that domain (don't leave empty placeholders)
- **Cross-reference everything** — backing services table should reference which apps use them; app dependency tables should reference the backing services section
- **Include enough detail to deploy** — someone (or an agent) reading an app's section should have 80% of what they need to get that app running from scratch

---

## Phase 5: Next Steps — Configure Stakpak Autopilot

After writing `APPS.md`, the primary next step is **setting up Stakpak Autopilot** to continuously monitor and maintain the discovered applications. Manual one-off checks are useful for immediate issues, but autopilot ensures ongoing health.

### Default Recommendation: Stakpak Autopilot

Propose autopilot schedules **derived entirely from your discovery findings**. Analyze what you found (apps, backing services, infrastructure, delivery pipelines, observability gaps) and design schedules that address the specific risks and operational needs of *this* environment.

> Now that I understand your apps, let's set up `stakpak autopilot` to keep them running and healthy. Based on what I found, here are the schedules I recommend:

### Designing Schedules from Discovery

**Your discovery findings are the input. Schedules are the output.** For each significant finding, ask: "What could go wrong here, and how would I detect it early?"

Think about:
- **What's critical?** Customer-facing apps, databases with no replicas, single points of failure — these need frequent checks
- **What's fragile?** Services with known issues, manual deployment steps, missing health checks — these need monitoring
- **What's drifting?** IaC-managed resources, multi-environment setups, config that's injected at deploy time — these need periodic reconciliation
- **What's expiring?** Certificates, secrets, credentials without rotation policies — these need proactive alerting
- **What's invisible?** Services without observability, backing services with no health checks, costs with no tracking — these need visibility

Don't limit yourself to generic categories. If you discovered something unique about the environment (e.g., a cron job that processes payments nightly, a staging environment that's 3 versions behind prod, a manually-created CloudFront distribution), design a schedule specifically for it.

**Schedule frequency should match risk:** A customer-facing API health check might run every 5 minutes; a weekly cost review is fine for non-critical spend tracking. Let the criticality of what you found drive the cadence.

**Always include an `appsmd-refresh` schedule** — APPS.md is the agent's core knowledge base and must stay current. Re-run discovery periodically to add new services, update changed configs, and mark anything no longer found as `[unreachable]` or `[removed]` (never delete entries, let the user review).

### Checks: Keep Autopilot Deterministic

Every schedule should use a **check script** (`--check`) whenever the trigger condition can be verified deterministically. Checks are lightweight shell scripts that run *before* waking the agent — if the check passes/fails (based on `trigger_on`), the agent runs; otherwise it sleeps. This avoids wasting agent steps (and RAM) on situations that don't need attention.

- Use `--trigger-on failure` (default) to wake the agent only when the check detects a problem
- Use `--trigger-on success` to wake the agent only when a precondition is met
- Write checks as simple scripts: curl a health endpoint, query a DB, check a file age, etc.
- Store checks in `~/.stakpak/checks/` on the target machine
- Use the `create` tool with remote path format (`user@host:/path`) to write check scripts to remote servers

### Schedule Design Guidelines

- **Stagger cron minutes** — never schedule multiple jobs at `:00`. Spread them across the hour to avoid resource spikes from concurrent agent runs
- **Name schedules descriptively** — the name should tell you what it monitors at a glance (e.g., `payments-db-backup` not `backup-check-1`)
- **Prefer checks over always-run** — if you can write a 5-line shell script that detects the problem, use `--check` so the agent only wakes when needed
- **Start lean** — propose the minimum set that covers the crown jewels and highest-risk findings. The user can always add more later

### Proposal Format

Present schedules as a concrete plan the user can approve. Lead with the highest-priority items (based on what you discovered), and explain *why* each schedule exists by connecting it to a specific finding:

```
Based on what I found, here's what I'd set up:

1. **api-health** (every 3 min) — Your API is customer-facing with no redundancy
   Check: curl /health on api.example.com → only wake agent if it's down
2. **db-backup-verify** (daily 6am) — RDS instance has no cross-region replica
   Check: verify last snapshot < 24h old → only wake agent if backup is stale
3. **staging-drift** (daily 9:15am) — Staging is 2 minor versions behind prod
   No check — always run, compare deployed versions across environments
4. **apps-refresh** (weekly Monday 9am) — Keep APPS.md current
   No check — always run, re-discover and update the application registry

Want me to configure these? I'll set up the schedules and optionally connect Slack for alerts.
```

### After User Approval

If the user approves, configure autopilot:

1. Add each schedule using `stakpak autopilot schedule add`
2. If Slack/Discord integration is desired, configure the channel
3. Start autopilot with `stakpak up`
4. Verify schedules are active with `stakpak autopilot status`

### Fallback: Immediate One-Off Tasks

Only offer manual/one-off approaches if:
- The user explicitly declines autopilot setup
- The user needs something done *right now* before autopilot is configured
- The task is truly one-time (e.g., "generate an architecture diagram")

Even then, frame it as: "I'll do this now, and we can also schedule it in autopilot for ongoing monitoring."

---

## Behavioral Rules

1. **Apps are the unit of understanding** — every piece of infrastructure exists to serve an application. If you find a database, the question is "which app uses this?" not "what databases exist?"
2. **Understand enough to deploy** — for each app: what does it do, what does it depend on, how do I build it, how do I run it, how do I know it's healthy, how do I ship a new version?
3. **Code is the source of truth** — IaC, configs, and live state can drift. When they conflict, note the discrepancy. Source code is the most reliable signal.
4. **Speed over perfection** — get 80% of the picture fast, refine later. APPS.md is a living document.
5. **Maximize autonomy, minimize interruptions** — automate discovery, batch questions, never run CLI commands in the foreground (use sandboxed subagents). If Docker is unavailable, skip CLI discovery and note the gap. Respect the user's time — if they skip questions, move on.
6. **Never expose secrets** — treat all credentials, tokens, and keys as radioactive
7. **Be honest about confidence** — clearly distinguish `[confirmed]` facts from `[inferred]` ones
8. **Parallelize aggressively** — use subagents for all independent discovery tasks. Read-only by default — discovery must not modify infrastructure state (except writing APPS.md at the end).
9. **Fail gracefully** — if a discovery subagent fails or times out, note the gap and continue with what you have
10. **Don't assume source code access** — the current directory is just one signal. If it lacks app source code, extract app identities from IaC/manifests and flag that source-level analysis requires access to the source repos.
11. **Think like an operator** — you will be maintaining these apps. Frame every finding as "will I need this at 3am?" Build APPS.md incrementally for large environments.
