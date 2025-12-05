use crate::config::{AppConfig, ConfigFile, ProfileConfig, ProviderType};
use crate::apikey_auth::prompt_for_api_key;
use stakpak_shared::models::integrations::anthropic::{AnthropicConfig, AnthropicModel};
use stakpak_shared::models::integrations::gemini::{GeminiConfig, GeminiModel};
use stakpak_shared::models::integrations::openai::{OpenAIConfig, OpenAIModel};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType};
use crossterm::cursor::MoveTo;
use crossterm::execute;
use std::io::{self, stdout, Write};
use std::path::PathBuf;
use toml;

#[derive(Clone, Copy, PartialEq)]
enum Provider {
    Stakpak,
    Anthropic,
    OpenAI,
    Google,
}

impl Provider {
    fn as_str(&self) -> &'static str {
        match self {
            Provider::Stakpak => "Stakpak",
            Provider::Anthropic => "Anthropic",
            Provider::OpenAI => "OpenAI",
            Provider::Google => "Google",
        }
    }
}

fn render_step(step: &str, is_active: bool, is_completed: bool) {
    let indicator = if is_active {
        "\x1b[1;33m▲\x1b[0m"
    } else if is_completed {
        "\x1b[1;32m◆\x1b[0m"
    } else {
        "│"
    };
    let color = if is_active {
        "\x1b[1;36m"
    } else if is_completed {
        "\x1b[1;32m"
    } else {
        "\x1b[0m"
    };
    print!("{} {}{}\x1b[0m", indicator, color, step);
}

fn select_provider() -> Option<Provider> {
    let providers = [
        (Provider::Stakpak, "Stakpak (recommended)"),
        (Provider::Anthropic, "Anthropic"),
        (Provider::OpenAI, "OpenAI"),
        (Provider::Google, "Google"),
    ];
    
    let mut selected = 0;
    let mut search_input = String::new();
    
    enable_raw_mode().ok()?;
    
    print!("\r\n");
    print!("\x1b[1;36mAdd credential\x1b[0m\r\n");
    print!("\r\n");
    
    loop {
        // Save cursor
        print!("\x1b[s");
        
        // Render content
        render_step("Select provider", true, false);
        print!("\r\n");
        
        // Always show search line
        print!("  \x1b[90mSearch: {}\x1b[0m\r\n", search_input);
        
        let filtered: Vec<_> = if search_input.is_empty() {
            providers.iter().collect()
        } else {
            providers
                .iter()
                .filter(|(_, name)| {
                    name.to_lowercase().contains(&search_input.to_lowercase())
                })
                .collect()
        };
        
        if !filtered.is_empty() {
            if selected >= filtered.len() {
                selected = filtered.len() - 1;
            }
            
            for (idx, (_, name)) in filtered.iter().enumerate() {
                if idx == selected {
                    print!("  \x1b[1;32m●\x1b[0m \x1b[1;37m{}\x1b[0m\r\n", name);
                } else {
                    print!("  \x1b[90m○\x1b[0m \x1b[90m{}\x1b[0m\r\n", name);
                }
            }
        }
        
        print!("\r\n");
        print!("\x1b[1;37m↑/↓ to select • Enter: confirm • Type: to search\x1b[0m\r\n");
        let _ = stdout().flush();
        
        if let Ok(Event::Key(KeyEvent { code, kind: KeyEventKind::Press, modifiers, .. })) = event::read() {
            match code {
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    print!("\x1b[u");
                    print!("\x1b[0J");
                    let _ = stdout().flush();
                    disable_raw_mode().ok();
                    std::process::exit(130);
                }
                KeyCode::Enter => {
                    if !filtered.is_empty() {
                        // Restore cursor and clear content area
                        print!("\x1b[u");
                        print!("\x1b[0J");
                        let _ = stdout().flush();
                        disable_raw_mode().ok();
                        return Some(filtered[selected].0);
                    }
                }
                KeyCode::Up => {
                    if selected > 0 {
                        selected -= 1;
                    }
                    // Restore cursor and redraw
                    print!("\x1b[u");
                    print!("\x1b[0J");
                }
                KeyCode::Down => {
                    if selected < filtered.len().saturating_sub(1) {
                        selected += 1;
                    }
                    // Restore cursor and redraw
                    print!("\x1b[u");
                    print!("\x1b[0J");
                }
                KeyCode::Char(c) => {
                    search_input.push(c);
                    selected = 0;
                    // Restore cursor and redraw
                    print!("\x1b[u");
                    print!("\x1b[0J");
                }
                KeyCode::Backspace => {
                    search_input.pop();
                    selected = 0;
                    // Restore cursor and redraw
                    print!("\x1b[u");
                    print!("\x1b[0J");
                }
                KeyCode::Esc => {
                    print!("\x1b[u");
                    print!("\x1b[0J");
                    let _ = stdout().flush();
                    disable_raw_mode().ok();
                    return None;
                }
                _ => {}
            }
        } else {
            // If read fails, restore cursor
            print!("\x1b[u");
        }
    }
}

fn prompt_api_key(provider: Provider) -> Option<String> {
    // Clear screen and move to top using crossterm for reliability
    let _ = execute!(stdout(), Clear(ClearType::All), MoveTo(0, 0));
    
    println!();
    println!("\x1b[1;36mAdd credential\x1b[0m");
    println!();
    
    render_step("Select provider", false, true);
    println!();
    // Value in gray
    println!("  \x1b[90m{}\x1b[0m", provider.as_str());
    println!();
    
    render_step("Login method", false, true);
    println!();
    // Value in gray
    println!("  \x1b[90mManually enter API Key\x1b[0m");
    println!();
    
    let mut api_key = String::new();
    let mut show_required = false;

    loop {
        // Merge step indicator with input prompt
        print!("\x1b[1;33m▲\x1b[0m \x1b[1;36mEnter your API key:\x1b[0m ");
        
        if show_required {
            print!("\x1b[1;33m(Required)\x1b[0m ");
        }
        
        // Input color: Cyan
        print!("\x1b[1;36m");
        let _ = io::stdout().flush();
        
        api_key.clear();
        if io::stdin().read_line(&mut api_key).is_err() {
            return None;
        }
        print!("\x1b[0m"); // Reset color
        
        let trimmed = api_key.trim();
        if trimmed.is_empty() {
            show_required = true;
            // Move up one line (to the input line) and clear it so we can re-prompt
            print!("\x1b[1A\x1b[2K");
            continue;
        }
        
        return Some(trimmed.to_string());
    }
}

fn save_provider_api_key(
    config_path: &str,
    provider: Provider,
    api_key: String,
) -> Result<(), String> {
    let path = PathBuf::from(config_path);
    let mut config_file = match std::fs::read_to_string(&path) {
        Ok(content) => toml::from_str::<ConfigFile>(&content)
            .map_err(|e| format!("Failed to parse config: {}", e))?,
        Err(_) => ConfigFile::default(),
    };
    
    let byok_profile = config_file.profiles.entry("byok".to_string()).or_insert_with(|| {
        ProfileConfig {
            provider: Some(ProviderType::Local),
            ..ProfileConfig::default()
        }
    });
    
    byok_profile.provider = Some(ProviderType::Local);
    
    match provider {
        Provider::Anthropic => {
            byok_profile.anthropic = Some(AnthropicConfig {
                api_key: Some(api_key),
                api_endpoint: None,
            });
            byok_profile.smart_model = Some(AnthropicModel::Claude45Sonnet.to_string());
            byok_profile.eco_model = Some(AnthropicModel::Claude45Haiku.to_string());
            byok_profile.recovery_model = Some(AnthropicModel::Claude45Haiku.to_string());
        }
        Provider::OpenAI => {
            byok_profile.openai = Some(OpenAIConfig {
                api_key: Some(api_key),
                api_endpoint: None,
            });
            byok_profile.smart_model = Some(OpenAIModel::GPT5.to_string());
            byok_profile.eco_model = Some(OpenAIModel::GPT5Mini.to_string());
            byok_profile.recovery_model = Some(OpenAIModel::GPT5Mini.to_string());
        }
        Provider::Google => {
            byok_profile.gemini = Some(GeminiConfig {
                api_key: Some(api_key),
                api_endpoint: None,
            });
            byok_profile.smart_model = Some(GeminiModel::Gemini3Pro.to_string());
            byok_profile.eco_model = Some(GeminiModel::Gemini25Flash.to_string());
            byok_profile.recovery_model = Some(GeminiModel::Gemini25Flash.to_string());
        }
        Provider::Stakpak => {
            return Err("Stakpak should use existing flow".to_string());
        }
    }
    
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    
    let config_str = toml::to_string_pretty(&config_file)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&path, config_str)
        .map_err(|e| format!("Failed to write config: {}", e))?;
    
    Ok(())
}

pub async fn run_onboarding(config: &mut AppConfig) {
    if let Some(provider) = select_provider() {
        match provider {
            Provider::Stakpak => {
                prompt_for_api_key(config).await;
            }
            _ => {
                if let Some(api_key) = prompt_api_key(provider) {
                    let config_path = if config.config_path.is_empty() {
                        AppConfig::get_config_path::<&str>(None).display().to_string()
                    } else {
                        config.config_path.clone()
                    };
                    if let Err(e) = save_provider_api_key(&config_path, provider, api_key) {
                        eprintln!("Failed to save API key: {}", e);
                        std::process::exit(1);
                    }
                    println!();
                    println!("\x1b[1;32m✓ API key saved successfully\x1b[0m");
                    println!();
                }
            }
        }
    }
}
