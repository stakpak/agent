---
description: "Remotely install Stakpak Autopilot on any reachable Linux host over SSH, with optional AWS/GCP/Azure VM discovery or provisioning: /remote-autopilot-install"
tags: [stakpak, autopilot, remote-install, ssh, aws, gcp, azure, linux]
---

You are an installer agent. Install Stakpak Autopilot on a remote Linux host by gathering all required inputs with `ask_user`, resolving a reachable SSH target, then passing those values to the generic bootstrap script in one remote execution.

The bootstrap script is vendor-neutral Linux. Keep cloud-provider API calls in this skill as target adapters. Target adapters discover or provision a VM, confirm SSH reachability, and return a normalized SSH target.

---

Input: {input}

---

## Target outcome

A remote Linux host has:

- Stakpak CLI installed
- Docker installed and usable by the target user
- Autopilot running as a systemd user service
- Auth profile configured with a valid model
- Notification channel configured, unless skipped
- Notification routing configured to a destination such as `#prod`, a chat ID, or a channel ID
- `stakpak autopilot status` healthy

Expected runtime:

- Existing Linux VM with Docker already present: ~30–60s
- Existing fresh Linux VM: ~90–120s
- Full cloud VM provisioning plus install: usually ~3–5 minutes, provider-dependent

---

## Architecture rule

Keep the workflow split into two layers:

| Layer | Responsibility | Vendor-specific? |
|---|---|---:|
| Target adapter | Find or provision a reachable Linux host and collect SSH details | Yes |
| Bootstrap script | Install/configure Stakpak Autopilot on that host | Provider-neutral |

Every target adapter must return the same normalized output:

```text
ssh_target=<user>@<host>[:port]
ssh_credential_method=default-keys|private-key|password
target_user=<linux-user>
private_key_path=<optional-local-path>
password=<optional-password>
provider=<ssh|aws|gcp|azure>
source_resource_id=<optional-provider-resource-id>
public_ip=<optional-public-ip>
private_ip=<optional-private-ip>
```

After an adapter returns this data, all providers use the same probe, install, and verify phases.

---

## Input collection pattern

Use `ask_user` as the primary UX mechanism for collecting choices and sensitive values.

Rules:

- Collect values first, then run the bootstrap script once.
- Use placeholders when describing commands to the user.
- Keep secrets masked in user-visible output.
- Prefer environment variables over command-line flags for secrets, because flags may appear in process listings.
- Use provider CLIs for local discovery/provisioning. Keep provider logic in target adapters and keep the bootstrap script generic.

---

## Phase 1 — Choose target source

If `{input}` is a valid SSH target like `user@host` or `user@host:port`, use the Plain SSH adapter and skip directly to Phase 3.

Otherwise ask:

```json
{
  "questions": [
    {
      "label": "Target",
      "question": "Where should Autopilot be installed?",
      "options": [
        { "label": "Use an existing SSH host", "value": "ssh-existing" },
        { "label": "Choose an existing AWS EC2 instance", "value": "aws-existing" },
        { "label": "Choose an existing GCP Compute Engine VM", "value": "gcp-existing" },
        { "label": "Choose an existing Azure VM", "value": "azure-existing" },
        { "label": "Provision a new AWS EC2 instance first", "value": "aws-new" },
        { "label": "Provision a new GCP VM first", "value": "gcp-new" },
        { "label": "Provision a new Azure VM first", "value": "azure-new" }
      ]
    }
  ]
}
```

Behavior:

- `ssh-existing`: ask for `user@host[:port]` and SSH credential method.
- `aws-existing`: use the AWS existing VM adapter.
- `gcp-existing`: use the GCP existing VM adapter.
- `azure-existing`: use the Azure existing VM adapter.
- `aws-new`, `gcp-new`, `azure-new`: explain the proposed VM defaults, ask for approval, provision, wait for SSH, then continue with the same install path.

If provider CLI credentials are unavailable or discovery returns an error, fall back to the Plain SSH adapter and ask the user for a reachable SSH target.

---

## Phase 2 — Target adapters

### Adapter contract

Each adapter must produce a reachable SSH target. Continue to installer configuration after the adapter has determined:

- SSH hostname/IP
- SSH username
- SSH credential method
- Whether the target user can run `sudo`
- Which cloud resource was selected or created, if applicable

Prefer public IPs for direct SSH. If only a private IP exists, ask how to connect:

- VPN/direct network path already available
- bastion/jump host
- provider-native tunnel/session manager
- choose another VM

If the connection path needs changes, stop and explain the required network or access update.

---

### Plain SSH adapter

Use when:

- `{input}` is already `user@host` or `user@host:port`
- the user chooses an existing SSH host
- provider discovery is unavailable or unnecessary

Ask for:

- SSH target: `user@host` or `user@host:port`
- SSH credential method: default keys, specific private key path, or password
- target Linux user, if different from the SSH user

Return:

```text
provider=ssh
ssh_target=<provided-target>
target_user=<ssh-user-or-custom-user>
```

---

### AWS existing EC2 adapter

Ask for:

- AWS region
- AWS CLI profile: default/current environment or custom

Run local discovery with `run_command`:

```bash
aws ec2 describe-instances \
  --region <region> \
  --filters "Name=instance-state-name,Values=running" \
  --query 'Reservations[].Instances[].{Id:InstanceId,Name:Tags[?Key==`Name`]|[0].Value,Type:InstanceType,PublicIp:PublicIpAddress,PrivateIp:PrivateIpAddress,Platform:PlatformDetails,ImageId:ImageId,KeyName:KeyName,State:State.Name,VpcId:VpcId,SubnetId:SubnetId}' \
  --output json
```

If a non-default profile is provided, include:

```bash
--profile <profile>
```

Transform results into `ask_user` options:

```text
<Name or unnamed> | <InstanceId> | <PublicIp or private-only> | <Type> | <Platform>
```

Each option value should contain:

```text
instance_id|public_ip|private_ip|name|key_name|image_id|platform|vpc_id|subnet_id
```

Ask for Linux SSH user. Suggested defaults:

- `ec2-user` for Amazon Linux/RHEL/CentOS/Rocky/AlmaLinux
- `ubuntu` for Ubuntu
- `admin` for Debian
- custom

Return:

```text
provider=aws
source_resource_id=<instance-id>
ssh_target=<ssh-user>@<public-ip-or-reachable-private-ip>
target_user=<ssh-user-or-custom-user>
```

---

### GCP existing Compute Engine adapter

Ask for:

- GCP project ID
- zone, or `all zones`
- gcloud account/configuration, if relevant

Run local discovery with `run_command`.

For one zone:

```bash
gcloud compute instances list \
  --project <project-id> \
  --zones <zone> \
  --filter='status=RUNNING' \
  --format='json(name,zone.basename(),machineType.basename(),networkInterfaces[0].accessConfigs[0].natIP,networkInterfaces[0].networkIP,disks[0].licenses)'
```

For all zones:

```bash
gcloud compute instances list \
  --project <project-id> \
  --filter='status=RUNNING' \
  --format='json(name,zone.basename(),machineType.basename(),networkInterfaces[0].accessConfigs[0].natIP,networkInterfaces[0].networkIP,disks[0].licenses)'
```

Transform results into `ask_user` options:

```text
<name> | <zone> | <natIP or private-only> | <machineType>
```

Each option value should contain:

```text
name|zone|public_ip|private_ip|machine_type|project_id
```

Ask for Linux SSH user. Suggested defaults:

- current local OS username if using `gcloud compute ssh`
- `ubuntu` for Ubuntu images
- `debian` or `admin` for Debian images
- custom

If the VM is private-only, ask whether to use IAP tunneling or another network path. For IAP, create a standard SSH tunnel first, then use the resulting reachable SSH target for remote commands.

Return:

```text
provider=gcp
source_resource_id=projects/<project-id>/zones/<zone>/instances/<name>
ssh_target=<ssh-user>@<public-ip-or-reachable-private-ip>
target_user=<ssh-user-or-custom-user>
```

---

### Azure existing VM adapter

Ask for:

- Azure subscription: current/default or custom subscription ID/name
- resource group, or `all resource groups`

Run local discovery with `run_command`.

For one resource group:

```bash
az vm list \
  --resource-group <resource-group> \
  --show-details \
  --query '[?powerState==`VM running`].{id:id,name:name,resourceGroup:resourceGroup,location:location,size:hardwareProfile.vmSize,publicIps:publicIps,privateIps:privateIps,os:storageProfile.osDisk.osType}' \
  --output json
```

For all resource groups:

```bash
az vm list \
  --show-details \
  --query '[?powerState==`VM running`].{id:id,name:name,resourceGroup:resourceGroup,location:location,size:hardwareProfile.vmSize,publicIps:publicIps,privateIps:privateIps,os:storageProfile.osDisk.osType}' \
  --output json
```

If a custom subscription is provided, run first:

```bash
az account set --subscription <subscription>
```

Transform results into `ask_user` options:

```text
<name> | <resourceGroup> | <publicIps or private-only> | <size> | <location>
```

Each option value should contain:

```text
id|name|resource_group|location|public_ip|private_ip|size|os
```

Ask for Linux SSH user. Suggested defaults:

- `azureuser` for many Azure quickstart images
- `ubuntu` for Ubuntu images
- `adminuser` if that is the image/default user selected during provisioning
- custom

If the VM is private-only, ask whether there is VPN/private network reachability, Azure Bastion, or another jump path. Direct `run_remote_command` requires a reachable SSH target.

Return:

```text
provider=azure
source_resource_id=<vm-resource-id>
ssh_target=<ssh-user>@<public-ip-or-reachable-private-ip>
target_user=<ssh-user-or-custom-user>
```

---

### New VM provisioning adapters

Provisioning is optional. Only provision after explicit user approval.

For all providers, ask for:

- region/location/zone
- instance size
- OS image family/distribution
- SSH key choice
- whether to create/open inbound SSH access
- whether to assign a public IP
- tags/labels
- cost sensitivity

Recommended defaults:

| Provider | Small default | OS default | SSH exposure |
|---|---|---|---|
| AWS | `t3.small` or `t3.micro` | Ubuntu LTS or Amazon Linux 2023 | restrict TCP/22 to caller IP |
| GCP | `e2-small` | Ubuntu LTS or Debian | restrict TCP/22 to caller IP or use IAP |
| Azure | `Standard_B1s` or `Standard_B2s` | Ubuntu LTS | restrict TCP/22 to caller IP |

#### Placement and capacity preflight

Before creating any VM, every cloud provisioning adapter must validate that the selected size/SKU is available in the selected placement. Choose a subnet, zone, or availability zone from the validated candidate list.

Use this provider-neutral flow:

1. Build a list of candidate placements in the selected region/location.
2. Filter candidates to placements that support the requested VM size/SKU/machine type.
3. Prefer placements that also satisfy networking requirements:
   - public IP support if direct SSH is needed
   - selected VPC/subnet/network
   - required firewall/security-group model
   - requested zone/availability-zone preference, if the user gave one
4. When the candidate list is empty for the requested size, ask the user to choose a different size or region/location.
5. When provisioning returns an availability/capacity error, clean up partial resources, retry a different validated placement once, then ask the user for the next retry decision.
6. Record the final placement in the summary and rollback notes.

Provider-specific checks:

**AWS EC2**

Validate instance type offerings before selecting a subnet/AZ:

```bash
aws ec2 describe-instance-type-offerings \
  --region <region> \
  --location-type availability-zone \
  --filters Name=instance-type,Values=<instance-type> \
  --query 'InstanceTypeOfferings[].Location' \
  --output json
```

Then select a subnet whose `AvailabilityZone` is in that supported AZ list:

```bash
aws ec2 describe-subnets \
  --region <region> \
  --filters Name=vpc-id,Values=<vpc-id> \
  --query 'Subnets[].{SubnetId:SubnetId,AvailabilityZone:AvailabilityZone,MapPublicIp:MapPublicIpOnLaunch,AvailableIpAddressCount:AvailableIpAddressCount}' \
  --output json
```

When `run-instances` returns `Unsupported`, `InsufficientInstanceCapacity`, or an AZ/placement error, delete any security group or other partial resources created for that attempt, pick another supported AZ/subnet, and retry once.

**GCP Compute Engine**

Validate the machine type in the target zone before creating the VM:

```bash
gcloud compute machine-types describe <machine-type> \
  --zone <zone> \
  --project <project-id> \
  --format=json
```

When the user chooses only a region, list candidate zones and test the machine type in each zone:

```bash
gcloud compute zones list \
  --project <project-id> \
  --filter='region:(<region>) status=UP' \
  --format='value(name)'
```

When creation returns `ZONE_RESOURCE_POOL_EXHAUSTED`, `does not exist in zone`, quota, or stockout errors, clean up partial resources, choose another validated zone, and retry once.

**Azure VM**

Validate the VM SKU in the selected location before creating the VM:

```bash
az vm list-skus \
  --location <location> \
  --size <vm-size> \
  --resource-type virtualMachines \
  --all \
  --output json
```

Inspect SKU restrictions and zone support before selecting an availability zone. When the SKU is restricted in the location or lacks the requested zone, ask for a different size/location or choose a supported zone.

When `az vm create` returns `SkuNotAvailable`, `AllocationFailed`, `OverconstrainedAllocationRequest`, or zone restriction errors, delete partial resources created for that attempt, choose another validated zone or size, and retry once.

After provisioning:

1. Wait until the VM is running.
2. Wait until SSH is reachable.
3. Return the same normalized adapter output.
4. Document created resource IDs for rollback.

Present cost/security implications clearly. Restrict SSH to the caller IP by default. Use `0.0.0.0/0` for SSH only after explicit user approval.

---

## Phase 3 — SSH credential method

Ask:

- Default SSH keys from `~/.ssh`
- Specific private key path
- Password

Then, if needed:

- `private-key`: ask for private key path with custom input
- `password`: ask for password with custom input

Use credential values only as tool parameters for `run_remote_command`, `run_remote_command_task`, `view`, `create`, or `str_replace` when remote access is needed.

Keep passwords, API keys, Slack tokens, Telegram tokens, Discord tokens, and private key contents in memory or approved secret channels.

---

## Phase 4 — Probe target

Run a read-only probe via `run_remote_command`:

```bash
echo "OS=$(. /etc/os-release && echo $ID)"; \
echo "USER=$(id -un)"; \
echo "ARCH=$(uname -m)"; \
command -v sudo >/dev/null && echo HAS_SUDO || true; \
command -v systemctl >/dev/null && echo HAS_SYSTEMD || true; \
command -v stakpak >/dev/null && echo HAS_STAKPAK || true; \
command -v docker >/dev/null && echo HAS_DOCKER || true; \
systemctl --user is-active stakpak-autopilot 2>/dev/null || true
```

Supported OS IDs for the current bootstrap script:

- `amzn`
- `ubuntu`
- `debian`
- `rhel`
- `fedora`
- `rocky`
- `almalinux`
- `centos`

Required target properties:

- Linux with `/etc/os-release`
- systemd available
- `sudo` available for the SSH/target user
- architecture: `x86_64`, `amd64`, `aarch64`, or `arm64`

If Autopilot is already active, ask:

- Reconfigure and restart
- Stop here

If the target OS requires additional support, stop and explain the bootstrap script update required before installation.

---

## Channel configuration guide

Use this section before collecting channel tokens. The goal is to help the user create or verify the messaging integration, then collect only the values required by the bootstrap script.

Supported channel types:

| Channel | Required values for installer | Setup source |
|---|---|---|
| Slack | bot token, app token, destination channel | Slack app manifest |
| Telegram | bot token, chat ID | Telegram BotFather and chat discovery |
| Discord | bot token, destination/channel ID | Discord developer portal |

### Slack channel setup

Slack requires a Socket Mode app with bot scopes and event subscriptions. Guide the user through creating the Slack app from the official Stakpak manifest, then collect the generated tokens.

Manifest source:

```text
https://github.com/stakpak/agent/blob/main/libs/gateway/src/channels/slack-manifest.yaml
```

Raw manifest source:

```text
https://raw.githubusercontent.com/stakpak/agent/main/libs/gateway/src/channels/slack-manifest.yaml
```

Slack app manifest:

```yaml
display_information:
  name: Stakpak
  description: AI agent for infrastructure operations
  background_color: "#1a1a2e"
features:
  bot_user:
    display_name: Stakpak
    always_online: true
  app_home:
    home_tab_enabled: false
    messages_tab_enabled: true
    messages_tab_read_only_enabled: false
oauth_config:
  scopes:
    bot:
      # Outbound messaging
      - chat:write
      # Reactions (receipt indicator)
      - reactions:read
      - reactions:write
      # Channel & conversation awareness
      - channels:read
      - groups:read
      - im:read
      - mpim:read
      # Message history (required for inbound messages)
      - channels:history
      - groups:history
      - im:history
      - mpim:history
      # @mention detection
      - app_mentions:read
settings:
  event_subscriptions:
    bot_events:
      - message.channels
      - message.groups
      - message.im
      - app_mention
  interactivity:
    is_enabled: true
  org_deploy_enabled: false
  socket_mode_enabled: true
  token_rotation_enabled: false
```

Slack setup steps:

1. Open Slack API Apps and choose **Create New App**.
2. Choose **From an app manifest**.
3. Select the workspace that should receive Autopilot messages.
4. Paste the Stakpak Slack manifest YAML.
5. Review and create the app.
6. Install the app to the workspace.
7. Copy the **Bot User OAuth Token** beginning with `xoxb-`; this becomes `SLACK_BOT_TOKEN`.
8. Open **Basic Information** and create an app-level token with the Socket Mode connection scope. Copy the token beginning with `xapp-`; this becomes `SLACK_APP_TOKEN`.
9. Invite the app to the destination channel, for example `/invite @Stakpak` in `#prod`.
10. Use the destination channel name or ID as `STAKPAK_NOTIFY_CHAT_ID`.

Installer variables for Slack:

```bash
STAKPAK_NOTIFY_CHANNEL='slack'
STAKPAK_NOTIFY_CHAT_ID='<slack-channel-name-or-id>'
SLACK_BOT_TOKEN='<xoxb-token>'
SLACK_APP_TOKEN='<xapp-token>'
```

Validation command after install:

```bash
stakpak autopilot channel test
```

Healthy Slack signal:

```text
✓ slack: <app-name> (workspace=<workspace-name>)
```

### Telegram channel setup

Collect:

- Telegram bot token from BotFather
- Telegram chat ID or destination

Installer variables for Telegram:

```bash
STAKPAK_NOTIFY_CHANNEL='telegram'
STAKPAK_NOTIFY_CHAT_ID='<telegram-chat-id>'
TELEGRAM_BOT_TOKEN='<telegram-bot-token>'
```

Validate with:

```bash
stakpak autopilot channel test
```

### Discord channel setup

Collect:

- Discord bot token
- Discord destination/channel ID

Installer variables for Discord:

```bash
STAKPAK_NOTIFY_CHANNEL='discord'
STAKPAK_NOTIFY_CHAT_ID='<discord-channel-id>'
DISCORD_BOT_TOKEN='<discord-bot-token>'
```

Validate with:

```bash
stakpak autopilot channel test
```

---

## Phase 5 — Gather installer configuration

Use `ask_user` to gather all installer configuration before running the script.

First gather non-sensitive choices:

- Auth provider: `stakpak` or `anthropic`
- Model: default `claude-opus-4-5-20251101`, `stakpak/claude-opus-4-6`, or custom
- Channels: Slack, Telegram, Discord, or skip

For Slack, Telegram, or Discord, follow the Channel configuration guide first, then gather sensitive/channel-specific values.

Always ask for:

- API key for the selected auth provider

If Slack selected, ask for:

- Slack bot token
- Slack app token
- Slack destination channel, e.g. `#prod`, `#alerts`

If Telegram selected, ask for:

- Telegram bot token
- Telegram chat ID / destination

If Discord selected, ask for:

- Discord bot token
- Discord destination/channel ID

Use one `ask_user` question per value with `allow_custom: true`.

If multiple channels are selected, choose the first selected channel as the default notification route unless the user chooses a different route.

Set:

```text
STAKPAK_NOTIFY_CHANNEL=<selected channel type>
STAKPAK_NOTIFY_CHAT_ID=<destination>
```

---

## Phase 6 — Invoke bootstrap script once

Build a single environment-variable prefix from all collected values. Include only relevant variables.

Always include:

```bash
STAKPAK_API_KEY='<api-key>'
STAKPAK_AUTH_PROVIDER='<provider>'
STAKPAK_MODEL='<model>'
```

If a notification route was configured:

```bash
STAKPAK_NOTIFY_CHANNEL='<slack|telegram|discord>'
STAKPAK_NOTIFY_CHAT_ID='<destination>'
```

For Slack:

```bash
SLACK_BOT_TOKEN='<slack-bot-token>'
SLACK_APP_TOKEN='<slack-app-token>'
```

For Telegram:

```bash
TELEGRAM_BOT_TOKEN='<telegram-bot-token>'
```

For Discord:

```bash
DISCORD_BOT_TOKEN='<discord-bot-token>'
```

Invoke the hosted generic bootstrap script:

```bash
curl -sSL https://raw.githubusercontent.com/noureldin-azzab/stakpak-autopilot-install/06de6725a85301d6bc026ad5a400f60e6974c477/autopilot-install.sh | \
  STAKPAK_API_KEY='<api-key>' \
  STAKPAK_AUTH_PROVIDER='<provider>' \
  STAKPAK_MODEL='<model>' \
  STAKPAK_NOTIFY_CHANNEL='<channel>' \
  STAKPAK_NOTIFY_CHAT_ID='<destination>' \
  SLACK_BOT_TOKEN='<bot-token>' \
  SLACK_APP_TOKEN='<app-token>' \
  sudo -E bash -s -- --target-user '<target-user>'
```

If channels are skipped:

```bash
curl -sSL https://raw.githubusercontent.com/noureldin-azzab/stakpak-autopilot-install/06de6725a85301d6bc026ad5a400f60e6974c477/autopilot-install.sh | \
  STAKPAK_API_KEY='<api-key>' \
  STAKPAK_AUTH_PROVIDER='<provider>' \
  STAKPAK_MODEL='<model>' \
  sudo -E bash -s -- --skip-channels --target-user '<target-user>'
```

Run with `run_remote_command` and a 300s timeout.

For development testing with a local candidate script, copy the candidate script to `/tmp/autopilot-install.sh` on the remote host using the remote file tools, then invoke:

```bash
STAKPAK_API_KEY='<api-key>' \
STAKPAK_AUTH_PROVIDER='<provider>' \
STAKPAK_MODEL='<model>' \
sudo -E bash /tmp/autopilot-install.sh --target-user '<target-user>'
```

Use placeholders in explanations. Print commands with placeholder secret values.

---

## Phase 7 — Verify

Run:

```bash
stakpak autopilot status
stakpak autopilot doctor
stakpak autopilot channel test
```

Healthy signals:

- Service active
- Server reachable
- Sandbox healthy
- Scheduler running
- Channel test succeeds, unless channels were skipped

When verification returns an error, inspect:

```bash
stakpak autopilot logs -n 100
systemctl --user status stakpak-autopilot --no-pager
```

Diagnose before retrying. Run repeated install attempts with an adjusted approach after two identical attempts.

---

## Phase 8 — Optional notification smoke test

Ask the user if they want to run a notification smoke test.

If yes, trigger a small autopilot action or channel test suitable for the configured channel. Confirm the user received the message.

---

## Phase 9 — Summary

Provide a concise install summary:

- Provider/source adapter used
- Cloud resource ID, if applicable
- SSH target, redacting sensitive connection details if needed
- Target OS and architecture
- Target user
- Auth provider and model
- Notification channel type and destination
- Autopilot health status
- Verification commands run
- Created cloud resources, if any
- Rollback commands/resources

Keep secrets masked in the summary.

Ask what to do next:

- Add scheduled checks
- Configure Slack/Telegram/Discord routing
- Harden the VM and SSH access
- Add cloud read-only credentials for Autopilot
- Generate an install report
- Tear down any newly provisioned test VM

---

## Rollback guidance

For an existing host, rollback usually means:

```bash
stakpak down
```

Then optionally remove Docker group membership, Docker, or the Stakpak CLI when the user asks. Preserve shared dependencies by default.

For a newly provisioned VM, summarize provider-specific cleanup commands and ask for approval before deleting resources.

Examples:

```bash
# AWS
aws ec2 terminate-instances --instance-ids <instance-id> --region <region>

# GCP
gcloud compute instances delete <name> --zone <zone> --project <project-id>

# Azure
az vm delete --ids <vm-resource-id>
```

Also clean up security groups, firewall rules, public IPs, disks, NICs, and SSH keys that were created specifically for this install.

---

## Known installer workarounds encoded in the script

- Amazon Linux/RHEL sudo `secure_path`: the script invokes Stakpak via absolute path.
- Docker group membership: the script restarts `user@$UID.service` after adding the target user to `docker`.
- Scheduled-agent provider config: the script writes `api_endpoint` and `model` into `[profiles.default]`.
- Notification routing: the script writes `[notifications]` with `channel`, `chat_id`, and `gateway_url`.

---

## References

- Generic bootstrap script: `https://raw.githubusercontent.com/noureldin-azzab/stakpak-autopilot-install/06de6725a85301d6bc026ad5a400f60e6974c477/autopilot-install.sh`
- Related rulebook guidance: `stakpak://stakpak.dev/v1/how-to-write-rulebooks.md`
- Command source path: `libs/api/src/commands/remote-autopilot-install.v1.md`
