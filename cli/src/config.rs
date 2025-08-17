use config::ConfigError;
use serde::{Deserialize, Serialize};
use stakpak_api::ClientConfig;
use std::collections::HashMap;
use std::fs::{create_dir_all, write};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProfileConfig {
    pub api_endpoint: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub machine_name: Option<String>,
    pub auto_append_gitignore: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConfigFile {
    pub profiles: HashMap<String, ProfileConfig>,
    pub settings: Settings,
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub api_endpoint: String,
    pub api_key: Option<String>,
    pub mcp_server_host: Option<String>,
    pub machine_name: Option<String>,
    pub auto_append_gitignore: Option<bool>,
    pub profile_name: String,
}

impl From<AppConfig> for ClientConfig {
    fn from(config: AppConfig) -> Self {
        ClientConfig {
            api_key: config.api_key.clone(),
            api_endpoint: config.api_endpoint.clone(),
        }
    }
}

fn get_config_path() -> String {
    format!(
        "{}/.stakpak/config.toml",
        std::env::var("HOME").unwrap_or_default()
    )
}

impl AppConfig {
    pub fn load(profile_name: &str) -> Result<Self, ConfigError> {
        let config_path: String = get_config_path();

        // Try to load existing config file
        let config_file = if Path::new(&config_path).exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| ConfigError::Message(format!("Failed to read config file: {}", e)))?;

            // Try to parse as new format first
            if let Ok(config_file) = toml::from_str::<ConfigFile>(&content) {
                config_file
            } else {
                // Try to parse as old format and migrate
                #[derive(Deserialize)]
                struct OldAppConfig {
                    pub api_endpoint: String,
                    pub api_key: Option<String>,
                    pub machine_name: Option<String>,
                    pub auto_append_gitignore: Option<bool>,
                }

                if let Ok(old_config) = toml::from_str::<OldAppConfig>(&content) {
                    // Migrate old config to new format
                    let mut profiles = HashMap::new();
                    profiles.insert(
                        "default".to_string(),
                        ProfileConfig {
                            api_endpoint: Some(old_config.api_endpoint),
                            api_key: old_config.api_key,
                        },
                    );

                    let migrated_config = ConfigFile {
                        profiles,
                        settings: Settings {
                            machine_name: old_config.machine_name,
                            auto_append_gitignore: old_config.auto_append_gitignore,
                        },
                    };

                    // Save the migrated config
                    let config_str = toml::to_string_pretty(&migrated_config).map_err(|e| {
                        ConfigError::Message(format!("Failed to serialize migrated config: {}", e))
                    })?;
                    write(&config_path, config_str).map_err(|e| {
                        ConfigError::Message(format!("Failed to save migrated config: {}", e))
                    })?;

                    migrated_config
                } else {
                    return Err(ConfigError::Message(
                        "Failed to parse config file in both old and new formats".to_string(),
                    ));
                }
            }
        } else {
            // Create default config structure
            let mut profiles = HashMap::new();
            profiles.insert(
                "default".to_string(),
                ProfileConfig {
                    api_endpoint: Some("https://apiv2.stakpak.dev".to_string()),
                    api_key: None,
                },
            );

            ConfigFile {
                profiles,
                settings: Settings {
                    machine_name: None,
                    auto_append_gitignore: Some(true),
                },
            }
        };

        // Don't allow "all" as a profile to be loaded directly
        if profile_name == "all" {
            return Err(ConfigError::Message(
                "Cannot use 'all' as a profile name. It's reserved for defaults.".to_string(),
            ));
        }

        // Get the specified profile
        let profile = if let Some(profile) = config_file.profiles.get(profile_name) {
            profile.clone()
        } else {
            return Err(ConfigError::Message(format!(
                "Profile '{}' not found in configuration",
                profile_name
            )));
        };

        // Get defaults from "all" profile if it exists
        let all_profile = config_file.profiles.get("all");

        // Apply inheritance: profile values override "all" profile values
        let api_endpoint = profile
            .api_endpoint
            .or_else(|| all_profile.and_then(|all| all.api_endpoint.clone()))
            .unwrap_or_else(|| "https://apiv2.stakpak.dev".to_string());

        let api_key = profile
            .api_key
            .or_else(|| all_profile.and_then(|all| all.api_key.clone()));

        // Override with environment variables if present
        let api_key = std::env::var("STAKPAK_API_KEY").ok().or(api_key);
        let api_endpoint = std::env::var("STAKPAK_API_ENDPOINT").unwrap_or(api_endpoint);

        Ok(AppConfig {
            api_endpoint,
            api_key,
            mcp_server_host: None, // This can be added to profiles later if needed
            machine_name: config_file.settings.machine_name,
            auto_append_gitignore: config_file.settings.auto_append_gitignore,
            profile_name: profile_name.to_string(),
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let config_path: String = get_config_path();

        // Load existing config or create new one
        let mut config_file = if Path::new(&config_path).exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| format!("Failed to read config file: {}", e))?;
            toml::from_str::<ConfigFile>(&content)
                .map_err(|e| format!("Failed to parse config file: {}", e))?
        } else {
            ConfigFile {
                profiles: HashMap::new(),
                settings: Settings {
                    machine_name: None,
                    auto_append_gitignore: Some(true),
                },
            }
        };

        // Update the current profile
        config_file.profiles.insert(
            self.profile_name.clone(),
            ProfileConfig {
                api_endpoint: Some(self.api_endpoint.clone()),
                api_key: self.api_key.clone(),
            },
        );

        // Update settings
        config_file.settings = Settings {
            machine_name: self.machine_name.clone(),
            auto_append_gitignore: self.auto_append_gitignore,
        };

        if let Some(parent) = Path::new(&config_path).parent() {
            create_dir_all(parent).map_err(|e| format!("{}", e))?;
        }
        let config_str = toml::to_string_pretty(&config_file).map_err(|e| format!("{}", e))?;
        write(config_path, config_str).map_err(|e| format!("{}", e))?;

        Ok(())
    }
}
