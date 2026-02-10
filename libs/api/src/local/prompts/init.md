# Analyzing Infrastructure

Please analyze the infrastructure for me. Based on detected credentials and configuration files, focus your analysis on the relevant cloud providers and technologies present.

## Step 1: Detect Environment

First, identify what infrastructure is present:

### Cloud Provider Credentials
- **AWS**: Check for `~/.aws/credentials`, `~/.aws/config`, AWS environment variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_PROFILE`)
- **GCP**: Check for `~/.config/gcloud/`, GCP credentials file, `GOOGLE_APPLICATION_CREDENTIALS` environment variable
- **Azure**: Check for `~/.azure/`, Azure CLI configuration, `AZURE_SUBSCRIPTION_ID` environment variable

### Infrastructure as Code
- **Terraform**: Look for `*.tf` files, `.terraform/` directory, `terraform.tfstate`
- **CloudFormation**: Look for CloudFormation templates (`*.yaml`, `*.json` with AWS resources)
- **Pulumi**: Look for `Pulumi.yaml`, `Pulumi.*.yaml` files
- **CDK**: Look for `cdk.json`, CDK app files

### Container Orchestration
- **Kubernetes**: Look for `*.yaml` files in `k8s/`, `manifests/`, `.kube/config`
- **Helm**: Look for `Chart.yaml`, `values.yaml`, `charts/` directory
- **Docker**: Look for `Dockerfile`, `docker-compose.yaml`
- **ECS**: Check for ECS task definitions
- **GKE/EKS/AKS**: Check cloud-specific Kubernetes configurations

---

## Step 2: Provider-Specific Analysis

Based on detected credentials and configurations, perform targeted analysis:

### IF AWS Credentials Found:

#### AWS Account & Access
- List AWS profiles and regions in use
- Review IAM roles and policies
- Flag overly permissive security groups or policies
- Check for hardcoded AWS keys in code

#### AWS Compute Resources
- **EC2**: List instances, check utilization, identify stopped/idle instances
- **Lambda**: List functions, check runtimes (flag deprecated), review invocation patterns
- **ECS/EKS**: List clusters, services, task definitions, node groups
- **Auto Scaling**: Review ASG configurations

#### AWS Storage & Databases
- **S3**: List buckets, check encryption, versioning, public access settings
- **RDS**: List instances (engine, version, Multi-AZ), check backup retention
- **DynamoDB**: List tables, check capacity mode (provisioned vs on-demand)
- **ElastiCache**: List cache clusters

#### AWS Networking
- **VPC**: Map VPCs, subnets (public/private), route tables
- **Load Balancers**: List ALB/NLB/CLB, review SSL certificates
- **Security Groups**: Flag rules with 0.0.0.0/0 access
- **Route 53**: Review DNS zones
- **CloudFront**: List distributions

#### AWS Security & Compliance
- Check CloudTrail logging status
- Review AWS Config rules
- Check Security Hub findings
- Review GuardDuty alerts
- Verify Secrets Manager / Parameter Store usage

#### AWS Cost Analysis
- Estimate monthly costs by service
- Identify Reserved Instance opportunities
- Find Spot Instance candidates
- Flag unused resources (EBS volumes, snapshots, Elastic IPs, idle LBs)

---

### IF GCP Credentials Found:

#### GCP Project & Access
- List active GCP projects
- Review IAM roles and service accounts
- Check for overly broad permissions
- Verify project billing accounts

#### GCP Compute Resources
- **Compute Engine**: List VM instances, check machine types, utilization
- **Cloud Functions**: List functions, check runtimes
- **Cloud Run**: List services
- **GKE**: List clusters, node pools, workload configurations

#### GCP Storage & Databases
- **Cloud Storage**: List buckets, check lifecycle policies, IAM bindings
- **Cloud SQL**: List instances (engine, version, HA setup)
- **Firestore/Datastore**: Review database configurations
- **BigQuery**: List datasets and estimated costs

#### GCP Networking
- **VPC**: List networks, subnets, firewall rules
- **Cloud Load Balancing**: Review load balancers and backends
- **Cloud DNS**: Review DNS zones
- **Cloud CDN**: Check CDN configurations

#### GCP Security
- Check Cloud Audit Logs
- Review Security Command Center findings
- Verify Secret Manager usage

#### GCP Cost Analysis
- Estimate monthly costs
- Identify committed use discount opportunities
- Find preemptible instance candidates

---

### IF Azure Credentials Found:

#### Azure Subscription & Access
- List active subscriptions and resource groups
- Review IAM roles and service principals
- Check for overly permissive access

#### Azure Compute Resources
- **Virtual Machines**: List VMs, check sizes and utilization
- **Azure Functions**: List function apps
- **AKS**: List clusters and node pools
- **Container Instances**: List running containers

#### Azure Storage & Databases
- **Storage Accounts**: List accounts, check replication and access tiers
- **Azure SQL**: List databases, check service tiers
- **Cosmos DB**: Review databases and consistency levels

#### Azure Networking
- **Virtual Networks**: List VNets, subnets, NSGs
- **Load Balancers**: Review LB configurations
- **Application Gateway**: Check gateway configurations

#### Azure Security
- Check Azure Security Center recommendations
- Review Key Vault usage
- Verify Azure Monitor alerts

#### Azure Cost Analysis
- Estimate monthly costs
- Identify reservation opportunities

---

## Step 3: Infrastructure as Code Analysis

### IF Terraform Detected:
- Analyze `*.tf` files for provider configurations
- Review terraform version and provider version constraints
- Check state backend configuration (S3, GCS, Azure Storage, Terraform Cloud)
- Identify modules in use and their sources
- Flag hardcoded secrets or sensitive values
- Review variable definitions and tfvars files
- Check for workspace usage

### IF CloudFormation Detected:
- List active CloudFormation stacks
- Review stack parameters and outputs
- Check for drift detection
- Analyze templates for best practices

### IF Pulumi Detected:
- Review Pulumi project configuration
- Check backend configuration (Pulumi Cloud, S3, etc.)
- Analyze stack configurations

---

## Step 4: Kubernetes & Containers Analysis

### IF Kubernetes Manifests Found:
- Review namespaces and resource quotas
- Check deployments, StatefulSets, DaemonSets
- Review Services (ClusterIP, NodePort, LoadBalancer, Ingress)
- Analyze ConfigMaps and Secrets (flag base64-only secrets)
- Review RBAC (Roles, RoleBindings, ClusterRoles)
- Check resource requests and limits
- Review Pod Security Policies / Pod Security Standards
- Check for deprecated API versions

### IF Helm Charts Found:
- Review Chart.yaml and dependencies
- Analyze values.yaml files
- Check for Helm releases using `helm list` (if accessible)

### IF Docker Found:
- Review Dockerfile for best practices:
  - Multi-stage builds
  - Layer caching optimization
  - Security (running as non-root, scanning for vulnerabilities)
  - Base image choices (Alpine vs others)

---

## Step 5: Full Infrastructure Overview

Synthesize findings into:

### Architecture Summary
- Map main components (compute, storage, databases, networking)
- Identify high availability setup (multi-AZ, multi-region)
- Document disaster recovery approach
- Note backup strategies

### Security Posture
- Summarize encryption in transit and at rest
- Review secrets management approach
- Flag any publicly exposed resources
- Note compliance requirements (HIPAA, PCI-DSS, SOC 2, etc.)

### Networking Architecture
- Document public vs private subnet strategies
- Review VPN or Direct Connect / ExpressRoute / Cloud Interconnect setup
- Check network peering configurations

---

## Step 6: Cost Analysis & Optimization

Provide:
1. **Estimated Monthly Costs**: Break down by category (compute, storage, network, etc.)
2. **Quick Wins**: Immediate cost savings (delete unused resources)
3. **Medium-term Optimizations**: Reserved capacity, committed use, savings plans
4. **Long-term Recommendations**: Architecture changes for efficiency

---

## Step 7: Security & Reliability Improvements

Prioritized recommendations:
1. **Critical**: Security vulnerabilities, publicly exposed resources
2. **High**: Missing backups, single points of failure
3. **Medium**: Cost optimizations, deprecated versions
4. **Low**: Nice-to-have improvements

---

## Step 8: Deliverables

Provide:
1. **Executive Summary**: 2-3 paragraph overview
2. **Infrastructure Inventory**: Tables of key resources
3. **Cost Analysis**: Current spend + optimization opportunities with estimated savings
4. **Security Findings**: Critical issues to address
5. **Action Items**: Prioritized recommendations

---

## Step 9: Generate AGENTS.md (REQUIRED)

**IMPORTANT**: After completing the infrastructure analysis, AUTOMATICALLY create an `AGENTS.md` file in the current directory to document the project for the team. This file should contain project-specific information based on your analysis.

Create the `AGENTS.md` file with the following structure:

```markdown
# AGENTS.md

## Infrastructure Overview
- **Cloud Provider(s)**: [AWS / GCP / Azure / Multi-cloud]
- **Primary Region(s)**: [List regions]
- **Infrastructure as Code**: [Terraform / CloudFormation / Pulumi / etc.]
- **Container Orchestration**: [Kubernetes / ECS / Cloud Run / etc.]

## Key Components
- **Compute**: [Summary of instances, functions, containers]
- **Databases**: [RDS, Cloud SQL, etc.]
- **Storage**: [S3, GCS, blob storage]
- **Networking**: [VPCs, load balancers, DNS]

## Deployment Procedures

### Infrastructure Deployment
\`\`\`bash
# Terraform example
cd terraform/
terraform init
terraform plan
terraform apply

# CloudFormation example
aws cloudformation deploy --template-file template.yaml --stack-name my-stack
\`\`\`

### Application Deployment
\`\`\`bash
# Kubernetes example
kubectl apply -f k8s/manifests/
kubectl rollout status deployment/api

# ECS example
aws ecs update-service --cluster prod --service api --force-new-deployment
\`\`\`

## Monitoring & Observability
- **Dashboards**: [CloudWatch / Stackdriver / Azure Monitor URLs]
- **Logs**: [Log aggregation locations]
- **Alerts**: [PagerDuty / Slack / Email setup]

## Estimated Monthly Costs
- **Total**: ~$X,XXX/month
- **Breakdown**: [Compute: $XXX, Storage: $XXX, etc.]

## Cost Optimization Opportunities
- [ ] Reserved capacity (estimated savings: $XXX/month)
- [ ] Rightsizing over-provisioned resources
- [ ] Storage class optimization
- [ ] Delete unused resources

## Security & Compliance
- [MFA enforcement status]
- [Logging and audit trail setup]
- [Secrets management approach]
- [Compliance certifications if applicable]

## Disaster Recovery
- **Backup Strategy**: [Description]
- **RTO/RPO**: [Recovery objectives]
- **Tested**: [Last DR test date]

## Rollback Procedures

### Infrastructure Rollback
\`\`\`bash
# Terraform rollback example
cd terraform/
git checkout <previous-commit>
terraform plan
terraform apply

# CloudFormation rollback
aws cloudformation cancel-update-stack --stack-name my-stack
\`\`\`

### Application Rollback
\`\`\`bash
# Kubernetes rollback
kubectl rollout undo deployment/api

# ECS rollback
aws ecs update-service --cluster prod --service api --task-definition api:PREVIOUS_VERSION
\`\`\`

## Common Issues & Solutions

### Issue 1: [Common Problem]
**Symptoms**: [What users see]
**Solution**: [How to fix]

### Issue 2: [Another Problem]
**Symptoms**: [What users see]
**Solution**: [How to fix]

## Team Contacts
- **Infrastructure Lead**: [Name / Email]
- **On-Call**: [Rotation link]
- **Escalation**: [Who to contact for critical issues]
```

---

## Important Notes

- **Use read-only tools only**: Do NOT modify any live infrastructure during analysis
- **Respect credentials**: Only access resources you have permission to view
- **Be thorough but efficient**: Focus on actionable insights
- **Security first**: Flag security issues immediately
- **Cost-conscious**: Always provide optimization recommendations

---

## Analysis Strategy

1. **Start broad**: Detect what's present (credentials, config files, IaC)
2. **Go deep on what exists**: Don't waste time analyzing AWS if only GCP is used
3. **Cross-reference**: Terraform configs should match live infrastructure
4. **Validate assumptions**: If Terraform shows 10 instances but AWS shows 8, investigate
5. **Think holistically**: Consider dependencies (app needs database, needs network, needs monitoring)
6. **Always create AGENTS.md**: This is REQUIRED - document your findings in AGENTS.md for the team

---

## Workflow Summary

1. Detect environment (cloud providers, IaC, orchestration)
2. Analyze what's present (inventory, costs, security)
3. Provide findings and recommendations
4. **CREATE AGENTS.md file** with project-specific documentation
