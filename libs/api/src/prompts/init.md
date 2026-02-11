# Infrastructure Analysis

Analyze the infrastructure based on detected credentials and configurations. Focus only on what's present.

## Phase 1: Detection

Detect in parallel:

| Category | Check For |
|----------|-----------|
| **Cloud** | AWS (`~/.aws/`), GCP (`~/.config/gcloud/`), Azure (`~/.azure/`) |
| **IaC** | Terraform (`*.tf`), CloudFormation, Pulumi, CDK |
| **Containers** | Kubernetes manifests, Helm charts, Dockerfiles, ECS/GKE/EKS/AKS |

## Phase 2: Analysis (only for detected technologies)

### Cloud Resources
- **Inventory**: Compute, storage, databases, networking
- **Security**: IAM policies, public exposure, encryption, secrets management
- **Cost**: Monthly estimates, unused resources, optimization opportunities

### Infrastructure as Code
- Provider/version constraints, state backend, modules, hardcoded secrets

### Containers & Orchestration
- Workloads, services, RBAC, resource limits, deprecated APIs
- Dockerfile best practices (multi-stage, non-root, base images)

## Phase 3: Deliverables

1. **Executive Summary** (2-3 paragraphs)
2. **Resource Inventory** (tables)
3. **Security Findings** (prioritized: Critical → High → Medium → Low)
4. **Cost Analysis** (current spend + savings opportunities)
5. **Action Items** (prioritized recommendations)

## Phase 4: Generate AGENTS.md (REQUIRED)

Create `AGENTS.md` in the current directory with:

```markdown
# AGENTS.md

## Overview
- **Cloud**: [Provider(s), Region(s)]
- **IaC**: [Terraform/CloudFormation/etc.]
- **Orchestration**: [K8s/ECS/etc.]

## Key Components
[Compute, Databases, Storage, Networking summary]

## Deployment
\`\`\`bash
# Infrastructure
[IaC commands]

# Application
[Deploy commands]
\`\`\`

## Rollback
\`\`\`bash
[Rollback commands]
\`\`\`

## Costs
- **Monthly**: ~$X,XXX
- **Optimizations**: [List opportunities]

## Security
[Key findings and status]

## DR & Monitoring
- **Backups**: [Strategy]
- **Alerts**: [Setup]
```

## Rules
- **Read-only**: Do not modify infrastructure
- **Efficient**: Skip providers not in use
- **Cross-reference**: IaC should match live state
- **Always create AGENTS.md**
