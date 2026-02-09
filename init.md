# Infrastructure analysis request

Please analyze the infrastructure for me

## 1. AWS credentials and access

- check aws credentials 
- Identify which accounts and regions are in use.
- Flag any insecure or overly broad credential usage.

## 2. Instances and cost analysis

- List and summarize compute instances (EC2, ECS tasks, Lambda, etc.) in the relevant accounts/regions.
- Estimate or report current spend for compute (instances, reserved capacity, spot).
- Highlight overprovisioned, idle, or underused resources and suggest optimizations (rightsizing, reserved instances, spot, shutdown schedules).

## 3. Full infrastructure overview

- Map the main components: VPCs, subnets, load balancers, databases, storage, DNS, CDN.
- Summarize networking (public/private, peering, VPN).
- Note high availability, backups, and disaster recovery where visible.
- Call out security posture (security groups, NACLs, encryption, secrets management).

## 4. Deliverables

- A concise written summary of the infrastructure and findings.
- Cost analysis and concrete recommendations to reduce spend where possible.
- A short list of security and reliability improvements, if any.

Use read-only and listing tools only; do not change any live infrastructure.
