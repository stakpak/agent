use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use std::error::Error;

use crate::commands::auto_update::run_auto_update;
#[derive(Deserialize, Debug)]
pub struct Release {
    pub tag_name: String,
}

pub async fn check_update(current_version: &str) -> Result<(), Box<dyn Error>> {
    let release = get_latest_cli_version().await?;
    if current_version != release {
        let sep = "\x1b[1;34m═\x1b[0m".repeat(40); // Half-length for better proportions
        println!("\n\x1b[1;34m┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓\x1b[0m");
        println!(
            "\x1b[1;34m┃\x1b[0m\x1b[1;36m⮕ \x1b[1;37m Version Update Available!\x1b[0m\x1b[1;34m ┃\x1b[0m"
        );
        println!("\x1b[1;34m┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛\x1b[0m");
        println!(
            "\x1b[1;37m \x1b[1;33m{}\x1b[0m → \x1b[1;32m{}\x1b[0m",
            current_version, release
        );
        println!("\x1b[1;35m{}\x1b[0m", sep);
        println!("\x1b[1;37m Upgrade to access the latest features! 🚀\x1b[0m");
        println!("\x1b[1;35m{}\x1b[0m", sep);
    }

    Ok(())
}

pub async fn get_latest_cli_version() -> Result<String, Box<dyn Error>> {
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
    Ok(release.tag_name)
}

pub async fn auto_update() -> Result<(), Box<dyn Error>> {
    let latest_version = get_latest_cli_version().await?;
    let current_version = format!("v{}", env!("CARGO_PKG_VERSION"));
    if current_version != latest_version {
        println!(
            "\n🚀 Update available!  \x1b[1;37m\x1b[1;33m{}\x1b[0m → \x1b[1;32m{}\x1b[0m ✨\n",
            current_version, latest_version
        );
        println!("Would you like to update? (y/n)");
        let mut input = String::new();
        if let Err(e) = std::io::stdin().read_line(&mut input) {
            eprintln!("Failed to read input: {}", e);
            return Ok(());
        }
        if input.trim() == "y" {
            run_auto_update().await?;
            std::process::exit(0);
        } else if input.trim() == "n" {
            println!("Update cancelled!");
            println!("Proceeding to open Stakpak Agent...")
        } else {
            println!("Invalid input! Please enter y or n.");
        }
    }
    Ok(())
}
