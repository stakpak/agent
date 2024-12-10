use config::{Config, ConfigError, File};
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, write};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    pub api_key: Option<String>,
}

fn get_config_path() -> String {
    format!(
        "{}/.stakpak/config.toml",
        std::env::var("HOME").unwrap_or_default()
    )
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let config_path: String = get_config_path();

        let config = Config::builder()
            .add_source(File::with_name(&config_path).required(false))
            .build()
            .unwrap_or_else(|_| Config::default());

        config.try_deserialize()
    }

    pub fn save(&self) -> Result<(), String> {
        let config_path: String = get_config_path();

        if let Some(parent) = Path::new(&config_path).parent() {
            create_dir_all(parent).map_err(|e| format!("{}", e))?;
        }
        let config_str = toml::to_string_pretty(self).map_err(|e| format!("{}", e))?;
        write(config_path, config_str).map_err(|e| format!("{}", e))?;

        Ok(())
    }
}
