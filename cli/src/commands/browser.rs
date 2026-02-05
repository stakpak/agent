use crate::utils::plugins::{PluginConfig, execute_plugin_command, get_plugin_path};
use std::process::Command;

fn get_browser_config() -> PluginConfig {
    PluginConfig {
        name: "browser".to_string(),
        base_url: "https://github.com/stakpak/tab".to_string(),
        targets: vec![
            "linux-x86_64".to_string(),
            "darwin-x86_64".to_string(),
            "darwin-aarch64".to_string(),
            "windows-x86_64".to_string(),
        ],
        version: None,
        repo: Some("tab".to_string()),
        owner: Some("stakpak".to_string()),
        version_arg: Some("version".to_string()),
    }
}

fn get_daemon_config() -> PluginConfig {
    PluginConfig {
        name: "browser-daemon".to_string(),
        base_url: "https://github.com/stakpak/tab".to_string(),
        targets: vec![
            "linux-x86_64".to_string(),
            "darwin-x86_64".to_string(),
            "darwin-aarch64".to_string(),
            "windows-x86_64".to_string(),
        ],
        version: None,
        repo: Some("tab".to_string()),
        owner: Some("stakpak".to_string()),
        version_arg: Some("--version".to_string()),
    }
}

pub async fn run_browser(args: Vec<String>) -> Result<(), String> {
    let browser_config = get_browser_config();
    let daemon_config = get_daemon_config();

    // Ensure daemon is available (downloaded if needed)
    get_plugin_path(daemon_config).await;

    // Get browser path and run it
    let browser_path = get_plugin_path(browser_config).await;
    let mut cmd = Command::new(&browser_path);
    cmd.args(&args);
    execute_plugin_command(cmd, "browser".to_string())
}
