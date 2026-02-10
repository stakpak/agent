# Analyzing Infrastructure

Please analyze the infrastructure for me

## 1. Cloud Provider Credentials and Access

- Check AWS credentials (access keys, roles, profiles)
- Check GCP credentials (service accounts, application default credentials)
- Check Azure credentials (service principals, managed identities)
- Identify which accounts, projects, subscriptions, and regions are in use
- Flag any insecure or overly broad credential usage
- Review IAM policies, roles, and permissions for least privilege

## 2. Infrastructure as Code (IaC)

### Terraform
- Locate and analyze Terraform configurations (*.tf files)
- Review terraform.tfvars and variable definitions
- Check Terraform state configuration (local, S3, Terraform Cloud)
- Identify workspaces and environments
- Review module usage and versioning
- Check for hardcoded credentials or secrets
- Validate provider configurations and versions
- Review resource naming conventions and tagging

### CloudFormation
- Locate and analyze CloudFormation templates (*.yaml, *.json)
- Review stack configurations and parameters
- Check for drift detection results
- Identify nested stacks and dependencies

### Pulumi / CDK / Other IaC
- Identify any Pulumi, AWS CDK, or other IaC tools in use
- Review configurations and deployment patterns

## 3. Container Orchestration

### Kubernetes
- Identify Kubernetes clusters (EKS, GKE, AKS, self-managed)
- Review cluster configurations and versions
- Analyze namespaces and resource quotas
- Check deployment manifests, StatefulSets, DaemonSets
- Review service definitions and ingress configurations
- Examine ConfigMaps and Secrets
- Review pod security policies and network policies
- Check resource requests and limits
- Identify autoscaling configurations (HPA, VPA, cluster autoscaler)
- Review RBAC configurations

### Helm
- Locate Helm charts and values files
- Review Helm releases and versions
- Check for chart repositories in use

### Docker Compose
- Locate docker-compose.yml files
- Review service definitions and networking

## 4. Compute Resources

### Virtual Machines
- List and summarize EC2 instances, GCE instances, Azure VMs
- Check instance types, sizes, and configurations
- Review auto-scaling groups and configurations
- Identify idle or underutilized instances
- Review reserved instances and savings plans

### Serverless
- List Lambda functions, Cloud Functions, Azure Functions
- Review function configurations, memory, timeout settings
- Check execution patterns and invocation metrics
- Review API Gateway configurations

### Container Services
- Analyze ECS tasks, Cloud Run services, Azure Container Instances
- Review task definitions and service configurations
- Check for Fargate vs EC2 launch types

## 5. Networking

- Map VPCs, VNets, subnets, and CIDR blocks
- Review security groups, NSGs, firewall rules
- Check NACLs and network policies
- Identify load balancers (ALB, NLB, CLB, Cloud Load Balancing)
- Review DNS configurations (Route53, Cloud DNS, Azure DNS)
- Check VPN and VPC peering configurations
- Identify NAT gateways and internet gateways
- Review CDN configurations (CloudFront, Cloud CDN, Azure CDN)

## 6. Storage

- List S3 buckets, Cloud Storage buckets, Azure Blob Storage
- Review bucket policies and access controls
- Check versioning, lifecycle policies, and encryption
- Identify EBS volumes, persistent disks, managed disks
- Review volume snapshots and backup configurations
- Check for unused or unattached volumes

## 7. Databases

- Identify RDS instances, Cloud SQL, Azure SQL
- Review database engine versions and configurations
- Check for read replicas and multi-AZ deployments
- Identify DynamoDB tables, Firestore, Cosmos DB
- Review backup and retention policies
- Check encryption at rest and in transit
- Identify connection pooling and caching layers

## 8. Monitoring and Logging

- Review CloudWatch, Cloud Monitoring, Azure Monitor configurations
- Check log aggregation (CloudWatch Logs, Cloud Logging, Log Analytics)
- Identify monitoring dashboards and alerting rules
- Review APM tools (DataDog, New Relic, Prometheus, Grafana)
- Check distributed tracing configurations

## 9. CI/CD and Deployment

- Identify CI/CD platforms (GitHub Actions, GitLab CI, Jenkins, CircleCI)
- Review pipeline configurations
- Check deployment strategies (blue/green, canary, rolling)
- Review artifact repositories and container registries
- Check environment promotion workflows

## 10. Security and Compliance

- Review secrets management (AWS Secrets Manager, HashiCorp Vault, Azure Key Vault)
- Check certificate management and expiration
- Review security scanning tools and results
- Identify compliance frameworks in use
- Check backup and disaster recovery procedures
- Review incident response and runbook documentation

## 11. Cost Analysis

- Estimate current monthly spend by service and resource type
- Identify top cost drivers
- Highlight overprovisioned or idle resources
- Suggest optimizations:
  - Rightsizing recommendations
  - Reserved instances or savings plans
  - Spot instances for non-critical workloads
  - Storage class optimization
  - Shutdown schedules for dev/test environments
  - Remove unused resources (EIPs, snapshots, volumes)

## 12. Deliverables

- Comprehensive written summary of infrastructure architecture
- Visual diagram of key components and data flows (if possible)
- Cost analysis with current spend and optimization recommendations
- Security and compliance findings with prioritized remediation steps
- High availability and disaster recovery assessment
- Infrastructure modernization recommendations
- Action items organized by priority (critical, high, medium, low)

## Important Notes

- Use read-only and listing tools only; do not change any live infrastructure
- Redact sensitive information (credentials, API keys, internal IPs)
- Focus on actionable insights and concrete recommendations
- Prioritize security, cost optimization, and reliability improvements
