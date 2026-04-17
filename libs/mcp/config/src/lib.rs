use stakpak_shared::paths::stakpak_home_dir;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum FileFormat {
    #[default]
    Toml,
    Json,
}

fn detect_format(path: &Path) -> FileFormat {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => FileFormat::Json,
        _ => FileFormat::Toml, // default
    }
}

/// Parsed MCP config file for read/write operations.
/// Uses BTreeMap to maintain stable alphabetical ordering when serializing.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfigFile {
    #[serde(rename = "mcpServers")]
    pub servers: BTreeMap<String, McpServerEntry>,
    #[serde(skip)]
    pub format: Option<FileFormat>,
}

/// A single MCP server entry in the config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServerEntry {
    CommandBased {
        command: String,
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        env: Option<HashMap<String, String>>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        disabled: bool,
    },
    UrlBased {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headers: Option<HashMap<String, String>>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        disabled: bool,
    },
}

impl McpServerEntry {
    pub fn is_disabled(&self) -> bool {
        match self {
            McpServerEntry::CommandBased { disabled, .. } => *disabled,
            McpServerEntry::UrlBased { disabled, .. } => *disabled,
        }
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        match self {
            McpServerEntry::CommandBased { disabled: d, .. } => *d = disabled,
            McpServerEntry::UrlBased { disabled: d, .. } => *d = disabled,
        }
    }

    pub fn entry_type(&self) -> &'static str {
        match self {
            McpServerEntry::CommandBased { .. } => "stdio",
            McpServerEntry::UrlBased { .. } => "http",
        }
    }

    /// Summary string for display (command+args or url)
    pub fn summary(&self) -> String {
        match self {
            McpServerEntry::CommandBased { command, args, .. } => {
                if args.is_empty() {
                    command.clone()
                } else {
                    format!("{} {}", command, args.join(" "))
                }
            }
            McpServerEntry::UrlBased { url, .. } => url.clone(),
        }
    }

    /// Truncate summary to max_len characters
    pub fn summary_truncated(&self, max_len: usize) -> String {
        let s = self.summary();
        if s.len() <= max_len {
            s
        } else {
            let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
            format!("{}...", truncated)
        }
    }
}

/// Search for an MCP proxy config file in standard locations.
///
/// Priority:
/// 1. `mcp.{toml,json}` (current directory)
/// 2. `.stakpak/mcp.{toml,json}` (current directory)
/// 3. `<stakpak_dir>/mcp.{toml,json}`
pub fn find_config_file() -> Result<String, String> {
    // Priority 1: mcp.{toml,json} in current directory
    let cwd_toml = PathBuf::from("mcp.toml");
    if cwd_toml.exists() {
        return Ok("mcp.toml".to_string());
    }

    let cwd_json = PathBuf::from("mcp.json");
    if cwd_json.exists() {
        return Ok("mcp.json".to_string());
    }

    // Priority 2: .stakpak/mcp.{toml,json} in current directory
    let cwd_stakpak = PathBuf::from(".stakpak");

    let cwd_stakpak_toml = cwd_stakpak.join("mcp.toml");
    if cwd_stakpak_toml.exists() {
        return Ok(cwd_stakpak_toml.to_string_lossy().to_string());
    }

    let cwd_stakpak_json = cwd_stakpak.join("mcp.json");
    if cwd_stakpak_json.exists() {
        return Ok(cwd_stakpak_json.to_string_lossy().to_string());
    }

    let stakpak_dir = stakpak_home_dir();

    // Priority 3: stakpak dir (typically ~/.stakpak/)
    let home_toml = stakpak_dir.join("mcp.toml");
    if home_toml.exists() {
        return Ok(home_toml.to_string_lossy().to_string());
    }

    let home_json = stakpak_dir.join("mcp.json");
    if home_json.exists() {
        return Ok(home_json.to_string_lossy().to_string());
    }

    Err(format!(
        "No MCP config found. Searched:\n\
         - ./mcp.toml, ./mcp.json\n\
         - ./.stakpak/mcp.toml, ./.stakpak/mcp.json\n\
         - {}/mcp.toml, {}/mcp.json",
        stakpak_dir.display(),
        stakpak_dir.display()
    ))
}

/// Resolve the config file path. If `explicit` is given, use it.
/// Otherwise search standard locations. If nothing found, return the
/// default path (`<stakpak_dir>/mcp.toml`)
pub fn resolve_config_path(explicit: Option<&str>) -> PathBuf {
    if let Some(path) = explicit {
        return PathBuf::from(path);
    }

    if let Ok(found) = find_config_file() {
        return PathBuf::from(found);
    }

    stakpak_home_dir().join("mcp.toml")
}

/// Load MCP config from a file.
pub fn load_config(path: &Path) -> Result<McpConfigFile, String> {
    if !path.exists() {
        return Ok(McpConfigFile::default());
    }

    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

    let format = detect_format(path);

    if content.trim().is_empty() {
        return Ok(McpConfigFile {
            servers: BTreeMap::new(),
            format: Some(format),
        });
    }

    let mut config: McpConfigFile = match format {
        FileFormat::Toml => toml::from_str(&content)
            .map_err(|e| format!("Failed to parse {} as TOML: {}", path.display(), e))?,
        FileFormat::Json => serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {} as JSON: {}", path.display(), e))?,
    };

    config.format = Some(format);
    Ok(config)
}

pub fn load_config_from_str(content: &str) -> Result<McpConfigFile, String> {
    match toml::from_str::<McpConfigFile>(content) {
        Ok(cfg) => Ok(cfg),
        Err(toml_err) => match serde_json::from_str::<McpConfigFile>(content) {
            Ok(cfg) => Ok(cfg),
            Err(json_err) => Err(format!(
                "Failed to parse config:\n- TOML: {}\n- JSON: {}",
                toml_err, json_err
            )),
        },
    }
}

/// Save MCP config to a file, creating parent directories if needed.
pub fn save_config(config: &McpConfigFile, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    let format = config.format.clone().unwrap_or_else(|| detect_format(path));

    let content = match format {
        FileFormat::Toml => {
            toml::to_string_pretty(config).map_err(|e| format!("Failed to serialize TOML: {e}"))?
        }
        FileFormat::Json => serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize JSON: {e}"))?,
    };

    fs::write(path, content).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

/// Add a server entry. Fails if name already exists.
pub fn add_server(
    config: &mut McpConfigFile,
    name: &str,
    entry: McpServerEntry,
) -> Result<(), String> {
    if name == "stakpak" || name == "paks" {
        return Err(format!("Cannot add server with reserved name '{name}'."));
    }

    if config.servers.contains_key(name) {
        return Err(format!(
            "Server '{name}' already exists. Use 'stakpak mcp remove {name}' first."
        ));
    }
    config.servers.insert(name.to_string(), entry);
    Ok(())
}

/// Remove a server entry. Fails if name not found.
pub fn remove_server(config: &mut McpConfigFile, name: &str) -> Result<McpServerEntry, String> {
    if name == "stakpak" || name == "paks" {
        return Err(format!("Cannot remove internal server '{name}'."));
    }

    config
        .servers
        .remove(name)
        .ok_or_else(|| format!("Server '{name}' not found."))
}

/// Toggle the disabled flag on a server entry.
pub fn set_server_disabled(
    config: &mut McpConfigFile,
    name: &str,
    disabled: bool,
) -> Result<(), String> {
    if name == "stakpak" || name == "paks" {
        return Err(format!("Cannot modify internal server '{name}'."));
    }

    let entry = config
        .servers
        .get_mut(name)
        .ok_or_else(|| format!("Server '{name}' not found."))?;
    entry.set_disabled(disabled);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_path(filename: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("stakpak-mcp-config-{unique}-{filename}"))
    }

    fn sample_config(format: Option<FileFormat>) -> McpConfigFile {
        let mut servers = BTreeMap::new();
        servers.insert(
            "github".to_string(),
            McpServerEntry::UrlBased {
                url: "https://api.githubcopilot.com/mcp".to_string(),
                headers: None,
                disabled: false,
            },
        );

        McpConfigFile { servers, format }
    }

    #[test]
    fn test_parse_toml_config() {
        let toml_str = r#"
[mcpServers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcpServers.github]
url = "https://api.githubcopilot.com/mcp"

[mcpServers.disabled-server]
command = "some-tool"
args = []
disabled = true
"#;
        let config: McpConfigFile = toml::from_str(toml_str).unwrap();
        assert_eq!(config.servers.len(), 3);
        assert!(config.servers.contains_key("context7"));
        assert!(config.servers.contains_key("github"));
        assert!(config.servers["disabled-server"].is_disabled());
        assert!(!config.servers["context7"].is_disabled());
    }

    #[test]
    fn test_add_remove_roundtrip() {
        let mut config = McpConfigFile::default();
        let entry = McpServerEntry::CommandBased {
            command: "npx".into(),
            args: vec!["-y".into(), "server".into()],
            env: None,
            disabled: false,
        };
        add_server(&mut config, "test", entry).unwrap();
        assert!(config.servers.contains_key("test"));

        // Duplicate fails
        let entry2 = McpServerEntry::UrlBased {
            url: "https://example.com".into(),
            headers: None,
            disabled: false,
        };
        assert!(add_server(&mut config, "test", entry2).is_err());

        remove_server(&mut config, "test").unwrap();
        assert!(!config.servers.contains_key("test"));
    }

    #[test]
    fn test_set_disabled() {
        let mut config = McpConfigFile::default();
        let entry = McpServerEntry::CommandBased {
            command: "npx".into(),
            args: vec![],
            env: None,
            disabled: false,
        };
        add_server(&mut config, "test", entry).unwrap();
        assert!(!config.servers["test"].is_disabled());

        set_server_disabled(&mut config, "test", true).unwrap();
        assert!(config.servers["test"].is_disabled());

        set_server_disabled(&mut config, "test", false).unwrap();
        assert!(!config.servers["test"].is_disabled());
    }

    #[test]
    fn test_detect_format() {
        assert!(matches!(
            detect_format(Path::new("mcp.toml")),
            FileFormat::Toml
        ));
        assert!(matches!(
            detect_format(Path::new("mcp.json")),
            FileFormat::Json
        ));
        assert!(matches!(
            detect_format(Path::new("mcp.unknown")),
            FileFormat::Toml
        ));
    }

    #[test]
    fn test_entry_type_and_summary() {
        let cmd = McpServerEntry::CommandBased {
            command: "npx".into(),
            args: vec!["-y".into(), "@upstash/context7-mcp".into()],
            env: None,
            disabled: false,
        };
        assert_eq!(cmd.entry_type(), "stdio");
        assert_eq!(cmd.summary(), "npx -y @upstash/context7-mcp");

        let url = McpServerEntry::UrlBased {
            url: "https://example.com/mcp".into(),
            headers: None,
            disabled: false,
        };
        assert_eq!(url.entry_type(), "http");
        assert_eq!(url.summary(), "https://example.com/mcp");
    }

    #[test]
    fn test_summary_truncated() {
        let entry = McpServerEntry::CommandBased {
            command: "long-command".into(),
            args: vec!["with".into(), "many".into(), "arguments".into()],
            env: None,
            disabled: false,
        };

        assert_eq!(entry.summary_truncated(100), entry.summary());
        assert_eq!(entry.summary_truncated(10), "long-co...");
        assert_eq!(entry.summary_truncated(2), "...");
    }

    #[test]
    fn test_remove_server_not_found() {
        let mut config = McpConfigFile::default();
        let err = remove_server(&mut config, "missing").unwrap_err();
        assert!(err.contains("Server 'missing' not found."));
    }

    #[test]
    fn test_set_disabled_not_found() {
        let mut config = McpConfigFile::default();
        let err = set_server_disabled(&mut config, "missing", true).unwrap_err();
        assert!(err.contains("Server 'missing' not found."));
    }

    #[test]
    fn test_resolve_config_path_explicit() {
        let explicit = "./custom/path/mcp.json";
        assert_eq!(resolve_config_path(Some(explicit)), PathBuf::from(explicit));
    }

    #[test]
    fn test_load_nonexistent_config_returns_default() {
        let path = temp_test_path("does-not-exist.toml");
        let config = load_config(&path).unwrap();
        assert!(config.servers.is_empty());
        assert!(config.format.is_none());
    }

    #[test]
    fn test_load_empty_config_returns_default_servers() {
        let path = temp_test_path("empty.toml");
        fs::write(&path, "\n").unwrap();

        let config = load_config(&path).unwrap();
        assert!(config.servers.is_empty());
        assert!(matches!(config.format, Some(FileFormat::Toml)));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_save_and_load_toml_roundtrip() {
        let path = temp_test_path("roundtrip.toml");
        let config = sample_config(Some(FileFormat::Toml));

        save_config(&config, &path).unwrap();
        let loaded = load_config(&path).unwrap();

        assert!(matches!(loaded.format, Some(FileFormat::Toml)));
        assert_eq!(loaded.servers.len(), 1);
        assert!(loaded.servers.contains_key("github"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_save_and_load_json_roundtrip() {
        let path = temp_test_path("roundtrip.json");
        let config = sample_config(Some(FileFormat::Json));

        save_config(&config, &path).unwrap();
        let loaded = load_config(&path).unwrap();

        assert!(matches!(loaded.format, Some(FileFormat::Json)));
        assert_eq!(loaded.servers.len(), 1);
        assert!(loaded.servers.contains_key("github"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_save_uses_path_extension_when_format_missing() {
        let path = temp_test_path("by-path-extension.json");
        let config = sample_config(None);

        save_config(&config, &path).unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.trim_start().starts_with('{'));

        let loaded = load_config(&path).unwrap();
        assert!(matches!(loaded.format, Some(FileFormat::Json)));

        let _ = fs::remove_file(path);
    }
}
