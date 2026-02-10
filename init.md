# Analyzing Infrastructure

Please analyze the infrastructure for me

## 1. AWS credentials and access

- Check AWS credentials (access keys, roles, profiles).
- Identify which accounts and regions are in use.
- Flag any insecure or overly broad credential usage.

## 2. Instances and cost analysis

- List and summarize compute instances (EC2, ECS tasks, Lambda, etc.) in the relevant accounts/regions.
- Estimate or report current spend for compute (instances, reserved capacity, spot).
- Highlight overprovisioned, idle, or underused resources and suggest optimizations (rightsizing, reserved instances, spot, shutdown schedules).

## 3. Infrastructure as Code (if present)

- **Terraform**: If *.tf files exist, analyze Terraform configs, modules, provider versions, state backend, and flag hardcoded secrets.
- **CloudFormation**: If *.yaml/*.json templates exist, review stacks and parameters.
- **Pulumi / CDK**: If present, summarize IaC patterns and deployment approach.

## 4. Kubernetes and containers (if present)

- **Kubernetes**: If k8s manifests, Helm charts, or cluster configs exist, review namespaces, deployments, services, ConfigMaps, Secrets, RBAC, and resource requests/limits.
- **Helm**: If Helm charts exist, review values files, releases, and chart dependencies.
- **Docker**: If Dockerfile or docker-compose.yml exist, review images, networking, and multi-stage builds.

## 5. Full infrastructure overview

- Map the main components: VPCs, subnets, load balancers, databases, storage, DNS, CDN.
- Summarize networking (public/private, peering, VPN).
- Note high availability, backups, and disaster recovery where visible.
- Call out security posture (security groups, NACLs, encryption, secrets management).

## 6. Deliverables

- A concise written summary of the infrastructure and findings.
- Cost analysis and concrete recommendations to reduce spend where possible.
- A short list of security and reliability improvements, if any.
- If IaC or Kubernetes is present, include a brief assessment of that stack.

Use read-only and listing tools only; do not change any live infrastructure.
