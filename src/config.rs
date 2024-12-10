pub struct Config {
    pub api_key: Option<String>,
}

pub fn load_config() -> Config {
    Config { api_key: None }
}

pub fn save_config(config: Config) {}
