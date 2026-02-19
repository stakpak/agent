use std::collections::HashMap;
use std::net::TcpListener;
use std::process::Command;

// ── Stakpak agent container constants ──────────────────────────────────────

/// The container image used for sandboxed agent sessions.
/// Override at runtime with STAKPAK_AGENT_IMAGE env var for local testing.
pub fn stakpak_agent_image() -> String {
    std::env::var("STAKPAK_AGENT_IMAGE")
        .unwrap_or_else(|_| format!("ghcr.io/stakpak/agent:v{}", env!("CARGO_PKG_VERSION")))
}

/// Default volume mounts for the stakpak agent container.
///
/// Single source of truth for every path the container needs.
/// Used by `WardenConfig::readonly_profile()`, `prepare_volumes()`,
/// and `build_dynamic_subagent_command()`.
pub fn stakpak_agent_default_mounts() -> Vec<String> {
    vec![
        // Stakpak config & credentials
        "~/.stakpak/config.toml:/home/agent/.stakpak/config.toml:ro".to_string(),
        "~/.stakpak/auth.toml:/home/agent/.stakpak/auth.toml:ro".to_string(),
        "~/.stakpak/data/local.db:/home/agent/.stakpak/data/local.db".to_string(),
        "~/.agent-board/data.db:/home/agent/.agent-board/data.db".to_string(),
        // Working directory
        "./:/agent:ro".to_string(),
        "./.stakpak:/agent/.stakpak".to_string(),
        // AWS — config read-only, SSO/STS cache writable for token refresh
        "~/.aws/config:/home/agent/.aws/config:ro".to_string(),
        "~/.aws/credentials:/home/agent/.aws/credentials:ro".to_string(),
        "~/.aws/sso:/home/agent/.aws/sso".to_string(),
        "~/.aws/cli:/home/agent/.aws/cli".to_string(),
        // GCP — credential files read-only, cache/logs/db writable for gcloud to function
        "~/.config/gcloud/active_config:/home/agent/.config/gcloud/active_config:ro".to_string(),
        "~/.config/gcloud/configurations:/home/agent/.config/gcloud/configurations:ro".to_string(),
        "~/.config/gcloud/application_default_credentials.json:/home/agent/.config/gcloud/application_default_credentials.json:ro".to_string(),
        "~/.config/gcloud/credentials.db:/home/agent/.config/gcloud/credentials.db:ro".to_string(),
        "~/.config/gcloud/access_tokens.db:/home/agent/.config/gcloud/access_tokens.db:ro".to_string(),
        "~/.config/gcloud/logs:/home/agent/.config/gcloud/logs".to_string(),
        "~/.config/gcloud/cache:/home/agent/.config/gcloud/cache".to_string(),
        // Azure — config read-only, MSAL token cache and session writable
        "~/.azure/config:/home/agent/.azure/config:ro".to_string(),
        "~/.azure/clouds.config:/home/agent/.azure/clouds.config:ro".to_string(),
        "~/.azure/azureProfile.json:/home/agent/.azure/azureProfile.json:ro".to_string(),
        "~/.azure/msal_token_cache.json:/home/agent/.azure/msal_token_cache.json".to_string(),
        "~/.azure/msal_http_cache.bin:/home/agent/.azure/msal_http_cache.bin".to_string(),
        "~/.azure/logs:/home/agent/.azure/logs".to_string(),
        // DigitalOcean & Kubernetes
        "~/.digitalocean:/home/agent/.digitalocean:ro".to_string(),
        "~/.kube:/home/agent/.kube:ro".to_string(),
        // SSH — config and keys read-only (useful for host aliases and remote connections)
        "~/.ssh:/home/agent/.ssh:ro".to_string(),
        // Aqua tool cache (named volume — persists downloaded CLIs across runs)
        "stakpak-aqua-cache:/home/agent/.local/share/aquaproj-aqua".to_string(),
    ]
}

/// Expand `~` to `$HOME` in a volume mount string.
pub fn expand_volume_path(volume: &str) -> String {
    if (volume.starts_with("~/") || volume.starts_with("~:"))
        && let Ok(home_dir) = std::env::var("HOME")
    {
        return volume.replacen("~", &home_dir, 1);
    }
    volume.to_string()
}

/// Check whether the host-side part of a volume mount is a Docker named volume
/// (as opposed to a bind mount path).
///
/// Named volumes don't start with `/`, `.`, or `~` and contain no `/`.
pub fn is_named_volume(host_part: &str) -> bool {
    !host_part.starts_with('/')
        && !host_part.starts_with('.')
        && !host_part.starts_with('~')
        && !host_part.contains('/')
}

/// Pre-create any Docker named volumes found in [`stakpak_agent_default_mounts`].
///
/// Running `docker volume create` is idempotent and prevents a race condition
/// when multiple sandbox containers first-use the same named volume in parallel.
pub fn ensure_named_volumes_exist() {
    for vol in stakpak_agent_default_mounts() {
        let host_part = vol.split(':').next().unwrap_or(&vol);
        if is_named_volume(host_part) {
            let _ = Command::new("docker")
                .args(["volume", "create", host_part])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContainerConfig {
    pub image: String,
    pub env_vars: HashMap<String, String>,
    pub ports: Vec<String>,       // Format: "host_port:container_port"
    pub extra_hosts: Vec<String>, // Format: "host:ip"
    pub volumes: Vec<String>,     // Format: "host_path:container_path"
}

pub fn find_available_port() -> Option<u16> {
    match TcpListener::bind("0.0.0.0:0") {
        Ok(listener) => listener.local_addr().ok().map(|addr| addr.port()),
        Err(_) => None,
    }
}

/// Checks if Docker is installed and accessible
pub fn is_docker_available() -> bool {
    Command::new("docker")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Checks if a Docker image exists locally
pub fn image_exists_locally(image: &str) -> Result<bool, String> {
    let output = Command::new("docker")
        .args(["images", "-q", image])
        .output()
        .map_err(|e| format!("Failed to execute docker images command: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(!stdout.is_empty())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(format!("Docker images command failed: {}", stderr))
    }
}

pub fn run_container_detached(config: ContainerConfig) -> Result<String, String> {
    let mut cmd = Command::new("docker");

    cmd.arg("run").arg("-d").arg("--rm");

    // Add ports
    for port_mapping in &config.ports {
        cmd.arg("-p").arg(port_mapping);
    }

    // Add environment variables
    for (key, value) in &config.env_vars {
        cmd.arg("-e").arg(format!("{}={}", key, value));
    }

    // Add extra hosts
    for host_mapping in &config.extra_hosts {
        cmd.arg("--add-host").arg(host_mapping);
    }

    // Add volumes
    for volume_mapping in &config.volumes {
        cmd.arg("-v").arg(volume_mapping);
    }

    // Add image
    cmd.arg(&config.image);

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to execute docker command: {}", e))?;

    if output.status.success() {
        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(container_id)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(format!("Docker command failed: {}", stderr))
    }
}

pub fn stop_container(container_id: &str) -> Result<(), String> {
    let output = Command::new("docker")
        .arg("stop")
        .arg(container_id)
        .output()
        .map_err(|e| format!("Failed to execute docker stop: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("No such container") {
            Ok(())
        } else {
            Err(format!("Failed to stop container: {}", stderr))
        }
    }
}

pub fn remove_container(
    container_id: &str,
    force: bool,
    remove_volumes: bool,
) -> Result<(), String> {
    let mut cmd = Command::new("docker");

    cmd.arg("rm");

    if force {
        cmd.arg("-f");
    }

    if remove_volumes {
        cmd.arg("-v");
    }

    cmd.arg(container_id);

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to execute docker rm: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("No such container") {
            Ok(())
        } else {
            Err(format!("Failed to remove container: {}", stderr))
        }
    }
}

pub fn get_container_host_port(container_id: &str, container_port: u16) -> Result<u16, String> {
    let output = Command::new("docker")
        .arg("port")
        .arg(container_id)
        .arg(container_port.to_string())
        .output()
        .map_err(|e| format!("Failed to get container port: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let port = stdout.split(':').next_back().unwrap_or("");
        Ok(port.parse().unwrap())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(format!("Failed to get container port: {}", stderr))
    }
}
