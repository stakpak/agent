use std::fmt::Write;
use std::path::{Path, PathBuf};

/// Discover cloud account configurations by reading config files directly.
/// No CLI calls — pure filesystem reads for speed. Cross-platform.
pub fn discover() -> String {
    let mut out = String::with_capacity(2048);

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return "(cannot determine home directory)\n".to_string(),
    };

    discover_aws(&home, &mut out);
    discover_gcp(&home, &mut out);
    discover_azure(&home, &mut out);
    discover_kubernetes(&home, &mut out);
    discover_docker_registries(&home, &mut out);
    discover_other_platforms(&home, &mut out);

    if out.is_empty() {
        return "(no cloud account configurations found)\n".to_string();
    }
    out
}

/// Parse AWS config/credentials to enumerate profiles, then call
/// `aws sts get-caller-identity` per profile (in parallel) to get
/// definitive account IDs and validate credentials are live.
fn discover_aws(home: &Path, out: &mut String) {
    let config_path = home.join(".aws/config");
    let creds_path = home.join(".aws/credentials");

    if !config_path.exists() && !creds_path.exists() {
        return;
    }

    let _ = writeln!(out, "### AWS\n");

    // Step 1: Parse config file for profile metadata
    let mut profiles: Vec<AwsProfile> = Vec::new();

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        let mut current_name: Option<String> = None;
        let mut region: Option<String> = None;
        let mut sso_url: Option<String> = None;
        let mut role_arn: Option<String> = None;
        let mut source_profile: Option<String> = None;
        let mut sso_account_id: Option<String> = None;

        let flush = |profiles: &mut Vec<AwsProfile>,
                     name: &Option<String>,
                     region: &Option<String>,
                     sso_url: &Option<String>,
                     role_arn: &Option<String>,
                     source_profile: &Option<String>,
                     sso_account_id: &Option<String>| {
            if let Some(n) = name {
                let method = if sso_url.is_some() {
                    "SSO"
                } else if role_arn.is_some() {
                    "assume-role"
                } else {
                    "credentials"
                };
                profiles.push(AwsProfile {
                    name: n.clone(),
                    method: method.to_string(),
                    region: region.clone(),
                    role_arn: role_arn.clone(),
                    source_profile: source_profile.clone(),
                    sso_account_id: sso_account_id.clone(),
                    // Will be filled by sts call
                    live_account_id: None,
                    live_arn: None,
                    auth_ok: None,
                });
            }
        };

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                flush(
                    &mut profiles,
                    &current_name,
                    &region,
                    &sso_url,
                    &role_arn,
                    &source_profile,
                    &sso_account_id,
                );
                let section = &trimmed[1..trimmed.len() - 1];
                current_name = Some(
                    section
                        .strip_prefix("profile ")
                        .unwrap_or(section)
                        .to_string(),
                );
                region = None;
                sso_url = None;
                role_arn = None;
                source_profile = None;
                sso_account_id = None;
            } else if let Some((key, value)) = trimmed.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "region" => region = Some(value.to_string()),
                    "sso_start_url" => sso_url = Some(value.to_string()),
                    "role_arn" => role_arn = Some(value.to_string()),
                    "source_profile" => source_profile = Some(value.to_string()),
                    "sso_account_id" => sso_account_id = Some(value.to_string()),
                    _ => {}
                }
            }
        }
        flush(
            &mut profiles,
            &current_name,
            &region,
            &sso_url,
            &role_arn,
            &source_profile,
            &sso_account_id,
        );
    }

    // Step 2: Call `aws sts get-caller-identity --profile X --output json` per profile in parallel
    if which::which("aws").is_ok() && !profiles.is_empty() {
        use std::thread;

        let handles: Vec<_> = profiles
            .iter()
            .map(|p| {
                let name = p.name.clone();
                thread::spawn(move || {
                    let output = std::process::Command::new("aws")
                        .args([
                            "sts",
                            "get-caller-identity",
                            "--profile",
                            &name,
                            "--output",
                            "json",
                        ])
                        .output();
                    (name, output)
                })
            })
            .collect();

        for handle in handles {
            if let Ok((name, output)) = handle.join()
                && let Some(profile) = profiles.iter_mut().find(|p| p.name == name)
            {
                match output {
                    Ok(o) if o.status.success() => {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                            profile.live_account_id = json
                                .get("Account")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            profile.live_arn = json
                                .get("Arn")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                        }
                        profile.auth_ok = Some(true);
                    }
                    _ => {
                        profile.auth_ok = Some(false);
                    }
                }
            }
        }
    }

    // Step 3: Format output
    for p in &profiles {
        // Best account ID: live > sso_account_id > extracted from role_arn
        let extracted_from_arn = p
            .role_arn
            .as_ref()
            .and_then(|arn| extract_account_from_arn(arn));
        let account_id = p
            .live_account_id
            .as_ref()
            .or(p.sso_account_id.as_ref())
            .or(extracted_from_arn.as_ref());

        let _ = write!(out, "- Profile: {}  method:{}", p.name, p.method);
        if let Some(acct) = account_id {
            let _ = write!(out, "  account:{}", acct);
        }
        if let Some(r) = &p.region {
            let _ = write!(out, "  region:{}", r);
        }
        if let Some(arn) = &p.live_arn {
            let _ = write!(out, "  arn:{}", arn);
        }
        if let Some(role) = &p.role_arn {
            let _ = write!(out, "  role:{}", role);
        }
        if let Some(src) = &p.source_profile {
            let _ = write!(out, "  source:{}", src);
        }
        match p.auth_ok {
            Some(true) => {
                let _ = write!(out, "  status:✓");
            }
            Some(false) => {
                let _ = write!(out, "  status:✗ auth-failed");
            }
            None => {} // aws CLI not available, don't show status
        }
        let _ = writeln!(out);
    }

    // Check env vars
    if let Ok(profile) = std::env::var("AWS_PROFILE") {
        let _ = writeln!(out, "- ENV: AWS_PROFILE={}", profile);
    }
    if let Ok(region) = std::env::var("AWS_REGION") {
        let _ = writeln!(out, "- ENV: AWS_REGION={}", region);
    }
    if let Ok(region) = std::env::var("AWS_DEFAULT_REGION") {
        let _ = writeln!(out, "- ENV: AWS_DEFAULT_REGION={}", region);
    }
    out.push('\n');
}

struct AwsProfile {
    name: String,
    method: String,
    region: Option<String>,
    role_arn: Option<String>,
    source_profile: Option<String>,
    sso_account_id: Option<String>,
    live_account_id: Option<String>,
    live_arn: Option<String>,
    auth_ok: Option<bool>,
}

/// Parse GCP config to enumerate projects and configurations.
fn discover_gcp(home: &Path, out: &mut String) {
    let gcloud_dir = home.join(".config/gcloud");
    if !gcloud_dir.exists() {
        return;
    }

    let _ = writeln!(out, "### GCP\n");

    // Read active config
    let active_config = gcloud_dir.join("active_config");
    let active = std::fs::read_to_string(&active_config)
        .ok()
        .map(|s| s.trim().to_string());

    if let Some(ref name) = active {
        let _ = writeln!(out, "- Active config: {}", name);
    }

    // Read properties from active config or default
    let configs_dir = gcloud_dir.join("configurations");
    if configs_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&configs_dir)
    {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("config_") {
                continue;
            }
            let config_name = name.strip_prefix("config_").unwrap_or(&name);
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                let project = extract_ini_value(&content, "project");
                let account = extract_ini_value(&content, "account");
                let region = extract_ini_value(&content, "region");
                let is_active = active.as_deref() == Some(config_name);
                let _ = write!(out, "- Config: {}", config_name);
                if is_active {
                    let _ = write!(out, " (active)");
                }
                if let Some(p) = project {
                    let _ = write!(out, "  project:{}", p);
                }
                if let Some(a) = account {
                    let _ = write!(out, "  account:{}", a);
                }
                if let Some(r) = region {
                    let _ = write!(out, "  region:{}", r);
                }
                let _ = writeln!(out);
            }
        }
    }

    // Check env vars
    if let Ok(project) = std::env::var("GCLOUD_PROJECT") {
        let _ = writeln!(out, "- ENV: GCLOUD_PROJECT={}", project);
    }
    if let Ok(project) = std::env::var("GOOGLE_CLOUD_PROJECT") {
        let _ = writeln!(out, "- ENV: GOOGLE_CLOUD_PROJECT={}", project);
    }
    out.push('\n');
}

/// Parse Azure CLI config to enumerate subscriptions.
fn discover_azure(home: &Path, out: &mut String) {
    let azure_dir = home.join(".azure");
    if !azure_dir.exists() {
        return;
    }

    let _ = writeln!(out, "### Azure\n");

    // azureProfile.json contains subscription info
    let profile_path = azure_dir.join("azureProfile.json");
    if let Ok(content) = std::fs::read_to_string(&profile_path)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(subs) = json.get("subscriptions").and_then(|s| s.as_array())
    {
        for sub in subs {
            let name = sub.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let id = sub.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let state = sub.get("state").and_then(|v| v.as_str()).unwrap_or("?");
            let is_default = sub
                .get("isDefault")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let tenant = sub.get("tenantId").and_then(|v| v.as_str()).unwrap_or("?");
            let _ = write!(
                out,
                "- {} ({})  state:{}  tenant:{}",
                name, id, state, tenant
            );
            if is_default {
                let _ = write!(out, "  (default)");
            }
            let _ = writeln!(out);
        }
    }
    out.push('\n');
}

/// Parse kubeconfig to enumerate contexts and clusters.
fn discover_kubernetes(home: &Path, out: &mut String) {
    // Check KUBECONFIG env var first, fall back to default
    let kubeconfig_paths = match std::env::var("KUBECONFIG") {
        Ok(val) => val.split(':').map(PathBuf::from).collect::<Vec<_>>(),
        Err(_) => vec![home.join(".kube/config")],
    };

    let mut found_any = false;
    for kc_path in &kubeconfig_paths {
        if !kc_path.exists() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(kc_path)
            && let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(&content)
        {
            if !found_any {
                let _ = writeln!(out, "### Kubernetes\n");
                found_any = true;
            }

            // Current context
            if let Some(current) = yaml.get("current-context").and_then(|v| v.as_str()) {
                let _ = writeln!(out, "- Current context: {}", current);
            }

            // List all contexts
            if let Some(contexts) = yaml.get("contexts").and_then(|v| v.as_sequence()) {
                for ctx in contexts {
                    let name = ctx.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let cluster = ctx
                        .get("context")
                        .and_then(|c| c.get("cluster"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let namespace = ctx
                        .get("context")
                        .and_then(|c| c.get("namespace"))
                        .and_then(|v| v.as_str());
                    let _ = write!(out, "- Context: {}  cluster:{}", name, cluster);
                    if let Some(ns) = namespace {
                        let _ = write!(out, "  namespace:{}", ns);
                    }
                    let _ = writeln!(out);
                }
            }
        }
    }
    if found_any {
        out.push('\n');
    }
}

/// Parse ~/.docker/config.json for configured registries (names only, never creds).
fn discover_docker_registries(home: &Path, out: &mut String) {
    let docker_config = home.join(".docker/config.json");
    if !docker_config.exists() {
        return;
    }

    if let Ok(content) = std::fs::read_to_string(&docker_config)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
    {
        let mut registries = Vec::new();

        // Check "auths" section
        if let Some(auths) = json.get("auths").and_then(|v| v.as_object()) {
            for key in auths.keys() {
                if !key.is_empty() {
                    registries.push(key.clone());
                }
            }
        }
        // Check "credHelpers" section
        if let Some(helpers) = json.get("credHelpers").and_then(|v| v.as_object()) {
            for key in helpers.keys() {
                if !registries.contains(key) {
                    registries.push(key.clone());
                }
            }
        }

        if !registries.is_empty() {
            let _ = writeln!(out, "### Container Registries\n");
            registries.sort();
            for reg in &registries {
                let _ = writeln!(out, "- {}", reg);
            }
            out.push('\n');
        }
    }
}

/// Check for other cloud platform CLIs and configs.
fn discover_other_platforms(home: &Path, out: &mut String) {
    let mut found = Vec::new();

    // Check config dirs
    let dir_checks: &[(&str, &str)] = &[
        (".wrangler", "Cloudflare"),
        (".vercel", "Vercel"),
        (".netlify", "Netlify"),
        (".fly", "Fly.io"),
    ];
    for (dir, name) in dir_checks {
        if home.join(dir).exists() {
            found.push(format!("- {} (~/{}/ exists)", name, dir));
        }
    }

    // Check CLIs via which
    let cli_checks: &[(&str, &str)] = &[
        ("doctl", "DigitalOcean"),
        ("hcloud", "Hetzner"),
        ("flyctl", "Fly.io"),
        ("railway", "Railway"),
        ("render", "Render"),
    ];
    for (cli, name) in cli_checks {
        if which::which(cli).is_ok() {
            found.push(format!("- {} ({} CLI installed)", name, cli));
        }
    }

    if !found.is_empty() {
        let _ = writeln!(out, "### Other Platforms\n");
        for entry in &found {
            let _ = writeln!(out, "{}", entry);
        }
        out.push('\n');
    }
}

/// Extract a value from an INI-style config file.
fn extract_ini_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some((k, v)) = trimmed.split_once('=')
            && k.trim() == key
        {
            let val = v.trim().to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

/// Extract the 12-digit account ID from an AWS ARN.
/// ARN format: arn:aws:iam::ACCOUNT_ID:role/RoleName
/// or:         arn:aws:sts::ACCOUNT_ID:assumed-role/...
fn extract_account_from_arn(arn: &str) -> Option<String> {
    let parts: Vec<&str> = arn.split(':').collect();
    if parts.len() >= 5 {
        let account = parts[4];
        // Account IDs are 12 digits
        if !account.is_empty() && account.chars().all(|c| c.is_ascii_digit()) {
            return Some(account.to_string());
        }
    }
    None
}
