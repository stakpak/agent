use flate2::read::GzDecoder;
use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Archive;
use zip::ZipArchive;

/// Configuration for a plugin download
pub struct PluginConfig {
    pub name: String,
    pub base_url: String,
    pub targets: Vec<String>,
    pub version: Option<String>,
}

/// Get the path to a plugin, downloading it if necessary
pub async fn get_plugin_path(config: PluginConfig) -> String {
    let config = PluginConfig {
        name: config.name,
        base_url: config.base_url.trim_end_matches('/').to_string(), // Remove trailing slash
        targets: config.targets,
        version: config.version,
    };

    // Get the target version from the server
    let target_version = match config.version.clone() {
        Some(version) => version,
        None => match get_latest_version(&config).await {
            Ok(version) => version,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to check latest version for {}: {}",
                    config.name, e
                );
                // Continue with existing logic if version check fails
                return get_plugin_path_without_version_check(&config).await;
            }
        },
    };

    // First check if plugin is available in PATH
    if let Ok(system_version) = get_version_from_command(&config.name, &config.name) {
        if is_same_version(&system_version, &target_version) {
            return config.name.clone();
        } else {
            // println!(
            //     "{} v{} is outdated (target: v{}), checking plugins directory...",
            //     config.name, system_version, target_version
            // );
        }
    }

    // Check if plugin already exists in plugins directory
    if let Ok(existing_path) = get_existing_plugin_path(&config.name)
        && let Ok(current_version) = get_version_from_command(&existing_path, &config.name)
    {
        if is_same_version(&current_version, &target_version) {
            return existing_path;
        } else {
            // println!(
            //     "{} {} is outdated (target: v{}), updating...",
            //     config.name, current_version, target_version
            // );
        }
    }

    // Try to download and install the latest version
    match download_and_install_plugin(&config).await {
        Ok(path) => {
            // println!(
            //     "Successfully installed {} v{} -> {}",
            //     config.name, target_version, path
            // );
            path
        }
        Err(e) => {
            eprintln!("Failed to download {}: {}", config.name, e);
            // Try to use existing version if available
            if let Ok(existing_path) = get_existing_plugin_path(&config.name) {
                eprintln!("Using existing {} version", config.name);
                existing_path
            } else if is_plugin_available(&config.name) {
                eprintln!("Using system PATH version of {}", config.name);
                config.name.clone()
            } else {
                eprintln!("No fallback available for {}", config.name);
                config.name.clone() // Last resort fallback
            }
        }
    }
}

/// Get plugin path without version checking (fallback function)
async fn get_plugin_path_without_version_check(config: &PluginConfig) -> String {
    // First check if plugin is available in PATH
    if is_plugin_available(&config.name) {
        return config.name.clone();
    }

    // Check if plugin already exists in plugins directory
    if let Ok(existing_path) = get_existing_plugin_path(&config.name) {
        return existing_path;
    }

    // Try to download and install plugin to ~/.stakpak/plugins
    match download_and_install_plugin(config).await {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Failed to download {}: {}", config.name, e);
            config.name.clone() // Fallback to system PATH (may not work)
        }
    }
}

/// Get version by running a command (can be plugin name or path)
fn get_version_from_command(command: &str, display_name: &str) -> Result<String, String> {
    let output = Command::new(command)
        .arg("version")
        .output()
        .map_err(|e| format!("Failed to run {} version command: {}", display_name, e))?;

    if !output.status.success() {
        return Err(format!("{} version command failed", display_name));
    }

    let version_output = String::from_utf8_lossy(&output.stdout);
    let full_output = version_output.trim();

    if full_output.is_empty() {
        return Err(format!("Could not determine {} version", display_name));
    }

    // Extract version from output like "warden v0.1.7 (https://github.com/stakpak/agent)"
    // Split by whitespace and take the second part (the version)
    let parts: Vec<&str> = full_output.split_whitespace().collect();
    if parts.len() >= 2 {
        Ok(parts[1].to_string())
    } else {
        // Fallback to full output if parsing fails
        Ok(full_output.to_string())
    }
}

/// Check if a plugin is available in the system PATH
pub fn is_plugin_available(plugin_name: &str) -> bool {
    get_version_from_command(plugin_name, plugin_name).is_ok()
}

/// Fetch the latest version from the remote server
async fn get_latest_version(config: &PluginConfig) -> Result<String, String> {
    let version_url = format!("{}/latest_version.txt", config.base_url);

    // Download the version file
    let client = create_tls_client(TlsClientConfig::default())?;
    let response = client
        .get(&version_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch latest version for {}: {}", config.name, e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch latest version for {}: HTTP {}",
            config.name,
            response.status()
        ));
    }

    let version_text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read version response: {}", e))?;

    Ok(version_text.trim().to_string())
}

/// Compare two version strings
fn is_same_version(current: &str, latest: &str) -> bool {
    let current_clean = current.strip_prefix('v').unwrap_or(current);
    let latest_clean = latest.strip_prefix('v').unwrap_or(latest);

    current_clean == latest_clean
}

/// Check if plugin binary already exists in plugins directory and get its version
fn get_existing_plugin_path(plugin_name: &str) -> Result<String, String> {
    let home_dir =
        std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;

    let stakpak_dir = PathBuf::from(&home_dir).join(".stakpak");
    let plugins_dir = stakpak_dir.join("plugins");

    // Determine the expected binary name based on OS
    let binary_name = if cfg!(windows) {
        format!("{}.exe", plugin_name)
    } else {
        plugin_name.to_string()
    };

    let plugin_path = plugins_dir.join(&binary_name);

    if plugin_path.exists() && is_executable(&plugin_path) {
        Ok(plugin_path.to_string_lossy().to_string())
    } else {
        Err(format!(
            "{} binary not found in plugins directory",
            plugin_name
        ))
    }
}

/// Download and install plugin binary to ~/.stakpak/plugins
async fn download_and_install_plugin(config: &PluginConfig) -> Result<String, String> {
    let home_dir =
        std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;

    let stakpak_dir = PathBuf::from(&home_dir).join(".stakpak");
    let plugins_dir = stakpak_dir.join("plugins");

    // Create directories if they don't exist
    fs::create_dir_all(&plugins_dir)
        .map_err(|e| format!("Failed to create plugins directory: {}", e))?;

    // Determine the appropriate download URL based on OS and architecture
    let (download_url, binary_name, is_zip) = get_download_info(config)?;

    let plugin_path = plugins_dir.join(&binary_name);

    // println!("Downloading {} plugin...", config.name);

    // Download the archive
    let client = create_tls_client(TlsClientConfig::default())?;
    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download {}: {}", config.name, e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download {}: HTTP {}",
            config.name,
            response.status()
        ));
    }

    let archive_bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download response: {}", e))?;

    // Extract the archive
    if is_zip {
        extract_zip(&archive_bytes, &plugins_dir)?;
    } else {
        extract_tar_gz(&archive_bytes, &plugins_dir)?;
    }

    // Make the binary executable on Unix systems
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&plugin_path)
            .map_err(|e| format!("Failed to get file metadata: {}", e))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&plugin_path, permissions)
            .map_err(|e| format!("Failed to set executable permissions: {}", e))?;
    }

    Ok(plugin_path.to_string_lossy().to_string())
}

/// Determine download URL and binary name based on OS and architecture
pub fn get_download_info(config: &PluginConfig) -> Result<(String, String, bool), String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    // Determine the current platform target
    let current_target = match (os, arch) {
        ("linux", "x86_64") => "linux-x86_64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("macos", "aarch64") => "darwin-aarch64",
        ("windows", "x86_64") => "windows-x86_64",
        _ => return Err(format!("Unsupported platform: {} {}", os, arch)),
    };

    // Check if this target is supported by the plugin
    if !config.targets.contains(&current_target.to_string()) {
        return Err(format!(
            "Plugin {} does not support target: {}",
            config.name, current_target
        ));
    }

    // Determine binary name and archive type
    let (binary_name, is_zip) = if current_target.starts_with("windows") {
        (format!("{}.exe", config.name), true)
    } else {
        (config.name.clone(), false)
    };

    // Construct download URL
    let extension = if is_zip { "zip" } else { "tar.gz" };
    let download_url = format!(
        "{}/{}/{}-{}.{}",
        config.base_url,
        config.version.clone().unwrap_or("latest".to_string()),
        config.name,
        current_target,
        extension
    );

    Ok((download_url, binary_name, is_zip))
}

/// Check if a file is executable
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            let permissions = metadata.permissions();
            return permissions.mode() & 0o111 != 0;
        }
    }

    #[cfg(windows)]
    {
        // On Windows, .exe files are executable
        return path.extension().map_or(false, |ext| ext == "exe");
    }

    false
}

/// Extract tar.gz archive
pub fn extract_tar_gz(archive_bytes: &[u8], dest_dir: &Path) -> Result<(), String> {
    let cursor = Cursor::new(archive_bytes);
    let tar = GzDecoder::new(cursor);
    let mut archive = Archive::new(tar);

    archive
        .unpack(dest_dir)
        .map_err(|e| format!("Failed to extract tar.gz archive: {}", e))?;

    Ok(())
}

/// Extract zip archive
pub fn extract_zip(archive_bytes: &[u8], dest_dir: &Path) -> Result<(), String> {
    let cursor = Cursor::new(archive_bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("Failed to read zip archive: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to access file {} in zip: {}", i, e))?;

        let outpath = match file.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };

        if file.is_dir() {
            fs::create_dir_all(&outpath)
                .map_err(|e| format!("Failed to create directory {}: {}", outpath.display(), e))?;
        } else {
            if let Some(p) = outpath.parent()
                && !p.exists()
            {
                fs::create_dir_all(p).map_err(|e| {
                    format!("Failed to create parent directory {}: {}", p.display(), e)
                })?;
            }
            let mut outfile = fs::File::create(&outpath)
                .map_err(|e| format!("Failed to create file {}: {}", outpath.display(), e))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to extract file {}: {}", outpath.display(), e))?;
        }

        // Set permissions on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                fs::set_permissions(&outpath, fs::Permissions::from_mode(mode)).map_err(|e| {
                    format!("Failed to set permissions for {}: {}", outpath.display(), e)
                })?;
            }
        }
    }

    Ok(())
}
