use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use semver::Version;
use serde::Deserialize;
use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
use std::error::Error;

use crate::commands::auto_update::run_auto_update;

/// Parse version string (with or without 'v' prefix) into semver Version
fn parse_version(version_str: &str) -> Option<Version> {
    let cleaned = version_str.strip_prefix('v').unwrap_or(version_str);
    Version::parse(cleaned).ok()
}

/// Check if remote version is newer than current version using semver
fn is_newer_version(current: &str, remote: &str) -> bool {
    match (parse_version(current), parse_version(remote)) {
        (Some(current_ver), Some(remote_ver)) => remote_ver > current_ver,
        // If parsing fails, fall back to string comparison (shouldn't happen with valid versions)
        _ => current != remote,
    }
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct LatestRelease {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub html_url: String,
    pub prerelease: bool,
    pub draft: bool,
    pub body: Option<String>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct ReleaseResponse {
    pub repository: String,
    pub stargazers_count: u64,
    pub latest_release: LatestRelease,
    pub cached_at: String,
    pub expires_at: String,
}

fn format_changelog(body: &str) -> String {
    let mut output = String::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    let mut is_first_section = true;

    while i < lines.len() {
        let trimmed_line = lines[i].trim();

        // Skip version header (## 0.3.1)
        if trimmed_line.starts_with("## ") {
            i += 1;
            continue;
        }

        // Skip "Released on" line
        if trimmed_line.starts_with("Released on") {
            i += 1;
            continue;
        }

        // Handle section headers (### Features, ### Maintenance, etc.)
        if trimmed_line.starts_with("### ") {
            let section_name = trimmed_line.strip_prefix("### ").unwrap_or("").trim();
            if !section_name.is_empty() {
                if !is_first_section {
                    output.push('\n');
                }
                // Ensure consistent indentation: exactly 2 spaces before bullet
                output.push_str("  ");
                output.push_str("\x1b[1;35m");
                output.push_str("● ");
                output.push_str(section_name);
                output.push_str(":\x1b[0m\n");
                is_first_section = false;
            }
            i += 1;
            continue;
        }

        // Handle list items (- item)
        if trimmed_line.starts_with("- ") {
            let item = trimmed_line.strip_prefix("- ").unwrap_or("").trim();
            if !item.is_empty() {
                output.push_str("    ");
                output.push_str("\x1b[1;37m");
                output.push('•');
                output.push_str(" \x1b[0;37m");
                output.push_str(item);
                output.push_str("\x1b[0m\n");
            }
            i += 1;
            continue;
        }

        // Skip empty lines
        if trimmed_line.is_empty() {
            i += 1;
            continue;
        }

        // Handle other content as regular text
        output.push_str("  \x1b[0;37m");
        output.push_str(trimmed_line);
        output.push_str("\x1b[0m\n");

        i += 1;
    }
    output.trim_end().to_string()
}

pub async fn check_update(current_version: &str) -> Result<(), Box<dyn Error>> {
    let release = get_latest_release().await?;
    if is_newer_version(current_version, &release.tag_name) {
        let sep = "\x1b[1;34m═\x1b[0m".repeat(40);
        println!("\n\x1b[1;34m┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓\x1b[0m");
        println!(
            "\x1b[1;34m┃\x1b[0m\x1b[1;36m⮕ \x1b[1;37m Version Update Available!\x1b[0m\x1b[1;34m ┃\x1b[0m"
        );
        println!("\x1b[1;34m┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛\x1b[0m");
        println!(
            "\x1b[1;37m \x1b[1;33m{}\x1b[0m → \x1b[1;32m{}\x1b[0m",
            current_version, release.tag_name
        );
        println!("\x1b[1;35m{}\x1b[0m", sep);

        if let Some(body) = &release.body
            && !body.trim().is_empty()
        {
            println!("\x1b[1;37m What's new in this update:\x1b[0m");
            println!("\x1b[1;35m{}\x1b[0m", sep);
            let changelog = format_changelog(body);
            println!("{}", changelog);
            println!("\x1b[1;35m{}\x1b[0m", sep);
            println!(
                "\x1b[1;37m View full changelog: \x1b[0m\x1b[1;36m{}\x1b[0m",
                release.html_url
            );
            println!("\x1b[1;35m{}\x1b[0m", sep);
        }

        println!("\x1b[1;37m Upgrade to access the latest features! 🚀\x1b[0m");
        println!("\x1b[1;35m{}\x1b[0m", sep);
    }

    Ok(())
}

pub async fn get_latest_release() -> Result<LatestRelease, Box<dyn Error>> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("update-checker"));

    let client = create_tls_client(TlsClientConfig::default().with_headers(headers))?;

    let url = "https://apiv2.stakpak.dev/github/releases".to_string();

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err("Failed to fetch release info".into());
    }

    let release_response: ReleaseResponse = response.json().await?;
    Ok(release_response.latest_release)
}

pub async fn get_latest_cli_version() -> Result<String, Box<dyn Error>> {
    let release = get_latest_release().await?;
    Ok(release.tag_name)
}

pub async fn auto_update() -> Result<(), Box<dyn Error>> {
    let release = get_latest_release().await?;
    let current_version = format!("v{}", env!("CARGO_PKG_VERSION"));
    if is_newer_version(&current_version, &release.tag_name) {
        println!(
            "\n🚀 Update available!  \x1b[1;37m\x1b[1;33m{}\x1b[0m → \x1b[1;32m{}\x1b[0m ✨\n",
            current_version, release.tag_name
        );

        if let Some(body) = &release.body
            && !body.trim().is_empty()
        {
            println!("\x1b[1;37m What's new in this update:\x1b[0m");
            println!("\x1b[1;36m{}\x1b[0m", "─".repeat(50));
            let changelog = format_changelog(body);
            println!("{}", changelog);
            println!("\x1b[1;36m{}\x1b[0m", "─".repeat(50));
            println!(
                "\x1b[1;37m View full changelog: \x1b[0m\x1b[1;36m{}\x1b[0m\n",
                release.html_url
            );
        }

        println!("Would you like to update? (y/n)");
        let mut input = String::new();
        if let Err(e) = std::io::stdin().read_line(&mut input) {
            eprintln!("Failed to read input: {}", e);
            return Ok(());
        }
        if input.trim() == "y" || input.trim().is_empty() {
            run_auto_update(false).await?;
        } else if input.trim() == "n" {
            println!("Update cancelled!");
            println!("Proceeding to open Stakpak Agent...")
        } else {
            println!("Invalid input! Please enter y or n.");
        }
    }
    Ok(())
}

/// Force auto-update without prompting (for ACP mode).
/// Returns true if an update was performed and the process should restart.
pub async fn force_auto_update() -> Result<bool, Box<dyn Error>> {
    let release = get_latest_release().await?;
    let current_version = format!("v{}", env!("CARGO_PKG_VERSION"));
    if is_newer_version(&current_version, &release.tag_name) {
        eprintln!(
            "🔄 Updating Stakpak: {} → {} ...",
            current_version, release.tag_name
        );
        run_auto_update(true).await?;
        // run_auto_update calls std::process::exit(0) on success,
        // so we only reach here if something went wrong
        Ok(true)
    } else {
        Ok(false)
    }
}
