use std::collections::HashMap;
use std::net::TcpListener;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ContainerConfig {
    pub image: String,
    pub env_vars: HashMap<String, String>,
    pub ports: Vec<String>,       // Format: "host_port:container_port"
    pub extra_hosts: Vec<String>, // Format: "host:ip"
    pub volumes: Vec<String>,     // Format: "host_path:container_path"
}

pub fn is_available_port(port: u16) -> bool {
    TcpListener::bind(format!("0.0.0.0:{}", port)).is_ok()
}

pub fn find_available_port() -> Option<u16> {
    match TcpListener::bind("0.0.0.0:0") {
        Ok(listener) => listener.local_addr().ok().map(|addr| addr.port()),
        Err(_) => None,
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
