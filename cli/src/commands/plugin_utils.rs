use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
use std::process::{Command, Stdio};

pub struct Plugin {
    pub plugin_name: &'static str,
    pub repo_owner: &'static str,
    pub repo_name: &'static str,
    pub artifact_prefix: &'static str,
    pub binary_name: &'static str,
}

pub async fn get_latest_github_release_version(
    owner: String,
    repo: String,
) -> Result<String, String> {
    let client = create_tls_client(TlsClientConfig::default());
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");

    let response = client?
        .get(url)
        .header("User-Agent", "stakpak-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch latest release version: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API returned: {}", response.status()));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub response: {}", e))?;

    json["tag_name"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No tag_name in release".to_string())
}

pub async fn get_plugin_existing_path(plugin_binary: String) -> Result<String, String> {
    let home_dir =
        std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;

    let plugin_path = std::path::PathBuf::from(&home_dir)
        .join(".stakpak")
        .join("plugins")
        .join(&plugin_binary);

    if plugin_path.exists() {
        Ok(plugin_path.to_string_lossy().to_string())
    } else {
        Err("Plugin not found".to_string())
    }
}

pub fn is_version_match(current: &str, target: &str) -> bool {
    let current_clean = current.strip_prefix('v').unwrap_or(current);
    let target_clean = target.strip_prefix('v').unwrap_or(target);
    current_clean == target_clean
}

pub fn execute_plugin_command(mut cmd: Command, plugin_name: String) -> Result<(), String> {
    cmd.stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::inherit());

    let status = cmd
        .status()
        .map_err(|e| format!("Failed to execute {} command: {}", plugin_name, e))?;

    if !status.success() {
        return Err(format!(
            "{} command failed with status: {}",
            plugin_name, status
        ));
    }

    std::process::exit(status.code().unwrap_or(1));
}

pub fn get_home_dir() -> Result<String, String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "HOME/USERPROFILE environment variable not set".to_string())
}

pub fn get_plugins_dir() -> Result<std::path::PathBuf, String> {
    let home_dir = get_home_dir()?;
    Ok(std::path::PathBuf::from(&home_dir)
        .join(".stakpak")
        .join("plugins"))
}

pub fn get_platform_suffix() -> Result<(&'static str, &'static str), String> {
    let platform = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        os => return Err(format!("Unsupported OS: {}", os)),
    };

    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        arch => return Err(format!("Unsupported architecture: {}", arch)),
    };

    Ok((platform, arch))
}

pub fn extract_tar_gz(
    data: &[u8],
    dest_dir: &std::path::Path,
    binary_name: &str,
) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use std::io::Cursor;
    use tar::Archive;

    let cursor = Cursor::new(data);
    let tar = GzDecoder::new(cursor);
    let mut archive = Archive::new(tar);

    for entry in archive
        .entries()
        .map_err(|e| format!("Failed to read archive: {}", e))?
    {
        let mut entry = entry.map_err(|e| format!("Failed to read archive entry: {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("Failed to get entry path: {}", e))?;

        if let Some(file_name) = path.file_name()
            && file_name == binary_name
        {
            let dest_path = dest_dir.join(file_name);
            entry
                .unpack(&dest_path)
                .map_err(|e| format!("Failed to extract binary: {}", e))?;
            return Ok(());
        }
    }

    Err("Binary not found in archive".to_string())
}

pub fn extract_zip(
    data: &[u8],
    dest_dir: &std::path::Path,
    binary_name: &str,
) -> Result<(), String> {
    use std::io::Cursor;
    use zip::ZipArchive;

    let cursor = Cursor::new(data);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("Failed to read zip archive: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry: {}", e))?;

        if file.name().ends_with(binary_name) {
            let dest_path = dest_dir.join(binary_name);
            let mut outfile = std::fs::File::create(&dest_path)
                .map_err(|e| format!("Failed to create output file: {}", e))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to write binary: {}", e))?;
            return Ok(());
        }
    }

    Err("Binary not found in archive".to_string())
}

pub async fn download_plugin_from_github(
    owner: &str,
    repo: &str,
    artifact_prefix: &str,
    binary_name: &str,
    version: Option<&str>,
) -> Result<String, String> {
    let plugins_dir = get_plugins_dir()?;
    std::fs::create_dir_all(&plugins_dir)
        .map_err(|e| format!("Failed to create plugins directory: {}", e))?;

    let plugin_path = plugins_dir.join(binary_name);
    if plugin_path.exists() {
        return Ok(plugin_path.to_string_lossy().to_string());
    }

    let (platform, arch) = get_platform_suffix()?;
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    let artifact_name = format!("{}-{}-{}.{}", artifact_prefix, platform, arch, extension);

    let download_url = match version {
        Some(v) => format!(
            "https://github.com/{}/{}/releases/download/{}/{}",
            owner, repo, v, artifact_name
        ),
        None => format!(
            "https://github.com/{}/{}/releases/latest/download/{}",
            owner, repo, artifact_name
        ),
    };

    eprintln!("{}", download_url);
    println!("Downloading {} binary...", artifact_prefix);

    let client = create_tls_client(TlsClientConfig::default())?;
    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download {}: {}", artifact_prefix, e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed: HTTP {}", response.status()));
    }

    let archive_bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download: {}", e))?;

    if extension == "zip" {
        extract_zip(&archive_bytes, &plugins_dir, binary_name)?;
    } else {
        extract_tar_gz(&archive_bytes, &plugins_dir, binary_name)?;
    }

    Ok(plugin_path.to_string_lossy().to_string())
}
