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

/// Canonical path of the AK knowledge store inside the agent container.
/// Both the default mount and the `AK_STORE`-override mount land here so the
/// in-container `ak` resolves to the same path regardless of host layout.
pub fn agent_knowledge_store_path() -> &'static str {
    "/home/agent/.stakpak/knowledge"
}

/// Host-side part of a volume mount (everything before the first `:`).
pub fn volume_host_part(vol: &str) -> &str {
    vol.split(':').next().unwrap_or(vol)
}

/// Container-side path of a volume mount (segment after the first `:`).
/// Falls back to the whole string when the format is unexpected.
pub fn volume_container_part(vol: &str) -> &str {
    vol.split(':').nth(1).unwrap_or(vol)
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
        // AK knowledge store — RW so sandboxed subagents can persist entries to the host.
        format!("~/.stakpak/knowledge:{}", agent_knowledge_store_path()),
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

/// Resolve the host-side AK knowledge store directory for a sandboxed subagent.
///
/// Returns:
/// - `Ok(None)` when `AK_STORE` is unset — the caller falls back to the default
///   mount entry from [`stakpak_agent_default_mounts`].
/// - `Ok(Some(path))` with an absolute, canonicalized host path when `AK_STORE`
///   is set and resolves cleanly.
/// - `Err(_)` with a message naming the offending path when `AK_STORE` is set but
///   cannot be canonicalized (e.g. broken symlink, missing parent).
///
/// Tilde-prefixed paths are expanded against `$HOME` before canonicalization so
/// values like `AK_STORE=~/my-store` work as users expect.
pub fn resolve_ak_store_for_sandbox() -> Result<Option<std::path::PathBuf>, String> {
    let raw = match std::env::var_os("AK_STORE") {
        Some(v) => v,
        None => return Ok(None),
    };

    let raw_str = raw.to_string_lossy().to_string();
    if raw_str.is_empty() {
        return Ok(None);
    }

    let expanded = if let Some(rest) = raw_str.strip_prefix("~/") {
        let home = std::env::var("HOME")
            .map_err(|_| format!("AK_STORE='{raw_str}' uses '~' but $HOME is not set"))?;
        std::path::PathBuf::from(home).join(rest)
    } else if raw_str == "~" {
        std::path::PathBuf::from(
            std::env::var("HOME")
                .map_err(|_| format!("AK_STORE='{raw_str}' uses '~' but $HOME is not set"))?,
        )
    } else {
        std::path::PathBuf::from(&raw_str)
    };

    // create_dir_all so a fresh AK_STORE path can be canonicalized without a
    // confusing NotFound, matching the host store's "create on first write".
    std::fs::create_dir_all(&expanded).map_err(|e| {
        format!(
            "AK_STORE='{raw_str}' could not be created at {}: {e}",
            expanded.display()
        )
    })?;

    let canonical = std::fs::canonicalize(&expanded).map_err(|e| {
        format!(
            "AK_STORE='{raw_str}' could not be resolved to an absolute path ({}): {e}",
            expanded.display()
        )
    })?;

    Ok(Some(canonical))
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

/// Warden CLI flags that pin the AK knowledge store inside the sandboxed
/// container. Returns an empty Vec when no host override is supplied — the
/// caller then relies on the default mount in [`stakpak_agent_default_mounts`].
pub fn warden_ak_store_args(host_knowledge_root: Option<&std::path::Path>) -> Vec<String> {
    match host_knowledge_root {
        Some(host_path) => {
            let target = agent_knowledge_store_path();
            vec![
                "--volume".to_string(),
                format!("{}:{target}", host_path.display()),
                "--env".to_string(),
                format!("AK_STORE={target}"),
            ]
        }
        None => Vec::new(),
    }
}

/// Pre-create any Docker named volumes found in [`stakpak_agent_default_mounts`].
///
/// Running `docker volume create` is idempotent and prevents a race condition
/// when multiple sandbox containers first-use the same named volume in parallel.
pub fn ensure_named_volumes_exist() {
    for vol in stakpak_agent_default_mounts() {
        let host_part = volume_host_part(&vol);
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

/// The platform warden uses for all containers.
/// Warden hardcodes `--platform linux/amd64` on every `docker run` / `docker create`,
/// so all image operations must target the same platform for consistency.
pub const WARDEN_PLATFORM: &str = "linux/amd64";

/// Checks if a Docker image for the warden platform (linux/amd64) exists locally.
///
/// Unlike [`image_exists_locally`], this uses `docker image inspect --platform`
/// to verify the correct architecture variant is cached. On Apple Silicon a plain
/// `docker images -q` would match a cached arm64 image, giving a false positive.
pub fn warden_image_exists_locally(image: &str) -> bool {
    Command::new("docker")
        .args(["image", "inspect", "--platform", WARDEN_PLATFORM, image])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Pull a Docker image for the warden platform (linux/amd64) with visible progress.
///
/// Inherits stdout/stderr so the caller sees Docker's native progress bars
/// (layer downloads, extraction, etc.). Returns an error if the pull fails.
pub fn pull_warden_image(image: &str) -> Result<(), String> {
    let status = Command::new("docker")
        .args(["pull", "--platform", WARDEN_PLATFORM, image])
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| format!("Failed to run docker pull: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Failed to pull image '{image}' for platform {WARDEN_PLATFORM}. \
             Check your network connection and that the image exists."
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // ENV_LOCK serializes tests that mutate process-wide env vars (AK_STORE,
    // HOME). All `unsafe { std::env::set_var / remove_var }` calls below are
    // sound because the lock guarantees no concurrent reader exists in this
    // suite.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn warden_ak_store_args_empty_when_no_override() {
        assert!(warden_ak_store_args(None).is_empty());
    }

    #[test]
    fn warden_ak_store_args_emits_volume_and_env_when_override_set() {
        let host = std::path::PathBuf::from("/tmp/custom-ak");
        let args = warden_ak_store_args(Some(&host));
        let target = agent_knowledge_store_path();
        assert_eq!(
            args,
            vec![
                "--volume".to_string(),
                format!("/tmp/custom-ak:{target}"),
                "--env".to_string(),
                format!("AK_STORE={target}"),
            ]
        );
    }

    #[test]
    fn volume_part_helpers_split_at_first_colon() {
        assert_eq!(volume_host_part("./:/agent:ro"), "./");
        assert_eq!(volume_container_part("./:/agent:ro"), "/agent");
        assert_eq!(volume_host_part("named-vol"), "named-vol");
        assert_eq!(volume_container_part("named-vol"), "named-vol");
    }

    #[test]
    fn knowledge_store_mount_present_and_rw() {
        let mounts = stakpak_agent_default_mounts();
        let suffix = format!(":{}", agent_knowledge_store_path());
        let entry = mounts
            .iter()
            .find(|v| v.ends_with(&suffix))
            .unwrap_or_else(|| panic!("knowledge store mount missing: {mounts:?}"));
        assert!(
            entry.starts_with("~/.stakpak/knowledge:"),
            "host side should be ~/.stakpak/knowledge: {entry}"
        );
        assert!(
            !entry.ends_with(":ro"),
            "knowledge store mount must be RW (no :ro suffix): {entry}"
        );
    }

    #[test]
    fn resolve_ak_store_returns_none_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("AK_STORE");
        }
        assert_eq!(resolve_ak_store_for_sandbox().unwrap(), None);
    }

    #[test]
    fn resolve_ak_store_expands_tilde() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let store_subdir = "ak-store-tilde-test";
        let expected = tmp.path().join(store_subdir);
        unsafe {
            std::env::set_var("HOME", tmp.path());
            std::env::set_var("AK_STORE", format!("~/{store_subdir}"));
        }
        let resolved = resolve_ak_store_for_sandbox().unwrap().unwrap();
        // canonicalize: macOS /var → /private/var.
        let expected_canonical = std::fs::canonicalize(&expected).unwrap();
        assert_eq!(resolved, expected_canonical);
        unsafe {
            std::env::remove_var("AK_STORE");
        }
    }

    #[test]
    fn resolve_ak_store_canonicalizes_relative_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let store_dir = tmp.path().join("relstore");
        std::fs::create_dir_all(&store_dir).unwrap();
        unsafe {
            std::env::set_var("AK_STORE", store_dir.to_str().unwrap());
        }
        let resolved = resolve_ak_store_for_sandbox().unwrap().unwrap();
        assert!(
            resolved.is_absolute(),
            "resolved path must be absolute: {resolved:?}"
        );
        let expected_canonical = std::fs::canonicalize(&store_dir).unwrap();
        assert_eq!(resolved, expected_canonical);
        unsafe {
            std::env::remove_var("AK_STORE");
        }
    }

    #[test]
    fn resolve_ak_store_creates_missing_directory() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let store_dir = tmp.path().join("does-not-exist-yet");
        assert!(!store_dir.exists());
        unsafe {
            std::env::set_var("AK_STORE", store_dir.to_str().unwrap());
        }
        let resolved = resolve_ak_store_for_sandbox().unwrap().unwrap();
        assert!(
            store_dir.exists(),
            "AK_STORE target should be created on resolve"
        );
        assert_eq!(resolved, std::fs::canonicalize(&store_dir).unwrap());
        unsafe {
            std::env::remove_var("AK_STORE");
        }
    }

    #[test]
    fn resolve_ak_store_fails_when_parent_unreachable() {
        let _guard = ENV_LOCK.lock().unwrap();
        // A non-directory file as parent → both mkdir and canonicalize fail,
        // so the resolver hits its error path.
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let bad = blocker.join("nested-store");
        unsafe {
            std::env::set_var("AK_STORE", bad.to_str().unwrap());
        }
        let err = resolve_ak_store_for_sandbox().unwrap_err();
        assert!(
            err.contains("AK_STORE="),
            "error should name the offending env value: {err}"
        );
        unsafe {
            std::env::remove_var("AK_STORE");
        }
    }
}
