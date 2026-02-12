use std::collections::HashMap;
use std::net::TcpListener;
use std::process::Command;

// ---------------------------------------------------------------------------
// Container image references — single source of truth
// ---------------------------------------------------------------------------

/// Base image reference for the Stakpak agent container (without tag).
const AGENT_IMAGE_REPO: &str = "ghcr.io/stakpak/agent";

/// Base image reference for the Warden sidecar container (without tag).
const WARDEN_SIDECAR_IMAGE_REPO: &str = "ghcr.io/stakpak/warden-sidecar";

/// Returns the fully-tagged agent image for the current CLI version.
/// Example: `ghcr.io/stakpak/agent:v0.3.40`
pub fn agent_image() -> String {
    format!("{}:v{}", AGENT_IMAGE_REPO, env!("CARGO_PKG_VERSION"))
}

/// Returns the fully-tagged warden sidecar image for the given warden version.
/// `warden_version` should include the leading `v` (e.g. `"v0.1.15"`).
/// Example: `ghcr.io/stakpak/warden-sidecar:v0.1.15`
pub fn warden_sidecar_image(warden_version: &str) -> String {
    format!("{}:{}", WARDEN_SIDECAR_IMAGE_REPO, warden_version)
}

/// Detect the installed warden version by running `warden version` (or a full
/// path to the binary). Returns the version token as printed by warden,
/// e.g. `"v0.1.15"`. Returns `None` if warden is not installed or the
/// version cannot be determined.
pub fn detect_warden_version(warden_path: &str) -> Option<String> {
    let output = Command::new(warden_path)
        .arg("version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output format: "warden v0.1.15 (https://github.com/stakpak/agent)"
    stdout.split_whitespace().nth(1).map(|s| s.to_string())
}

// ---------------------------------------------------------------------------

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
