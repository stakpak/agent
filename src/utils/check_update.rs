use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use std::error::Error;

#[derive(Deserialize, Debug)]
pub struct Release {
    pub tag_name: String,
}

pub async fn check_update(current_version: &str) -> Result<(), Box<dyn Error>> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("update-checker"));

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()?;

    let url = "https://api.github.com/repos/stakpak/cli/releases/latest".to_string();

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err("Failed to fetch release info".into());
    }

    let release: Release = response.json().await?;

    if current_version != release.tag_name {
        let sep = "\x1b[1;34m═\x1b[0m".repeat(40); // Half-length for better proportions
        println!("\n\x1b[1;34m┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓\x1b[0m");
        println!("\x1b[1;34m┃\x1b[0m\x1b[1;36m⮕ \x1b[1;37m Version Update Available!\x1b[0m\x1b[1;34m ┃\x1b[0m");
        println!("\x1b[1;34m┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛\x1b[0m");
        println!(
            "\x1b[1;37m \x1b[1;33m{}\x1b[0m → \x1b[1;32m{}\x1b[0m",
            current_version, release.tag_name
        );
        println!("\x1b[1;35m{}\x1b[0m", sep);
        println!("\x1b[1;37m Upgrade to access the latest features! 🚀\x1b[0m");
        println!("\x1b[1;35m{}\x1b[0m", sep);
    }

    Ok(())
}
