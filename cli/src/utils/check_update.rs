use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use semver::Version;
use serde::Deserialize;
use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
use std::error::Error;
use std::future::Future;

use crate::commands::auto_update::run_auto_update;
use crate::utils::cli_colors::CliColors;

/// Parse version string (with or without 'v' prefix) into semver Version
fn parse_version(version_str: &str) -> Option<Version> {
    let cleaned = version_str.strip_prefix('v').unwrap_or(version_str);
    Version::parse(cleaned).ok()
}

/// Check if remote version is newer than current version using semver
pub(crate) fn is_newer_version(current: &str, remote: &str) -> bool {
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

    let magenta = CliColors::magenta();
    let text = CliColors::text();
    let reset = CliColors::reset();

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
                output.push_str(magenta);
                output.push_str("● ");
                output.push_str(section_name);
                output.push(':');
                output.push_str(reset);
                output.push('\n');
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
                output.push_str(text);
                output.push('•');
                output.push(' ');
                output.push_str(item);
                output.push_str(reset);
                output.push('\n');
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
        output.push_str("  ");
        output.push_str(text);
        output.push_str(trimmed_line);
        output.push_str(reset);
        output.push('\n');

        i += 1;
    }
    output.trim_end().to_string()
}

pub async fn check_update(current_version: &str) -> Result<(), Box<dyn Error>> {
    let release = get_latest_release().await?;
    if is_newer_version(current_version, &release.tag_name) {
        let blue = CliColors::blue();
        let cyan = CliColors::cyan();
        let yellow = CliColors::yellow();
        let green = CliColors::green();
        let magenta = CliColors::magenta();
        let text = CliColors::text();
        let reset = CliColors::reset();

        let sep = format!("{}═{}", magenta, reset).repeat(40);
        println!("\n{}┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓{}", blue, reset);
        println!(
            "{}┃{}{}⮕ {} Version Update Available!{}{}┃{}",
            blue, reset, cyan, text, reset, blue, reset
        );
        println!("{}┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛{}", blue, reset);
        println!(
            "{} {}{}{} → {}{}{}",
            text, yellow, current_version, reset, green, release.tag_name, reset
        );
        println!("{}", sep);

        if let Some(body) = &release.body
            && !body.trim().is_empty()
        {
            println!("{} What's new in this update:{}", text, reset);
            println!("{}", sep);
            let changelog = format_changelog(body);
            println!("{}", changelog);
            println!("{}", sep);
            println!(
                "{} View full changelog: {}{}{}{}",
                text, reset, cyan, release.html_url, reset
            );
            println!("{}", sep);
        }

        println!(
            "{} Upgrade to access the latest features! 🚀{}",
            text, reset
        );
        println!("{}", sep);
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

async fn run_auto_update_if_newer<F, Fut>(
    current_version: &str,
    release: &LatestRelease,
    run_update: F,
) -> Result<bool, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    if is_newer_version(current_version, &release.tag_name) {
        run_update().await?;
        return Ok(true);
    }

    Ok(false)
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
    }

    run_auto_update_if_newer(&current_version, &release, || async {
        run_auto_update(true).await
    })
    .await
    .map_err(std::io::Error::other)
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    fn release(tag_name: &str) -> LatestRelease {
        LatestRelease {
            tag_name: tag_name.to_string(),
            name: format!("Stakpak {tag_name}"),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            html_url: "https://github.com/stakpak/agent/releases/latest".to_string(),
            prerelease: false,
            draft: false,
            body: Some("### Features\n- Faster updates".to_string()),
        }
    }

    #[tokio::test]
    async fn auto_update_runs_updater_when_release_is_newer() {
        let invoked = Arc::new(AtomicBool::new(false));
        let invoked_clone = Arc::clone(&invoked);

        let updated = run_auto_update_if_newer("v0.3.78", &release("v9.9.9"), || {
            invoked_clone.store(true, Ordering::SeqCst);
            async { Ok::<(), String>(()) }
        })
        .await
        .expect("auto update succeeds");

        assert!(updated);
        assert!(invoked.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn auto_update_skips_updater_when_release_is_not_newer() {
        let invoked = Arc::new(AtomicBool::new(false));
        let invoked_clone = Arc::clone(&invoked);

        let updated = run_auto_update_if_newer("v9.9.9", &release("v9.9.9"), || {
            invoked_clone.store(true, Ordering::SeqCst);
            async { Ok::<(), String>(()) }
        })
        .await
        .expect("auto update succeeds");

        assert!(!updated);
        assert!(!invoked.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn auto_update_logic_is_non_interactive() {
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            run_auto_update_if_newer("v0.3.78", &release("v9.9.9"), || async {
                Ok::<(), String>(())
            }),
        )
        .await;

        assert!(
            result.is_ok(),
            "auto-update logic should not wait for stdin"
        );
        let update_result = result.expect("timeout result");
        assert!(update_result.is_ok(), "auto-update logic should succeed");
    }
}
