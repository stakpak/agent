use std::io::{self, Write};
use std::path::PathBuf;

use clap::Subcommand;
use stakpak_gateway::{
    Gateway, GatewayCliFlags, GatewayConfig, build_channels, config::default_gateway_config_path,
};
use tokio_util::sync::CancellationToken;

use crate::config::AppConfig;

#[derive(Subcommand, PartialEq, Debug)]
pub enum GatewayCommands {
    /// Interactive channel setup wizard
    Init {
        #[arg(long, env = "TELEGRAM_BOT_TOKEN")]
        telegram_token: Option<String>,
        #[arg(long, env = "DISCORD_BOT_TOKEN")]
        discord_token: Option<String>,
        #[arg(long, env = "SLACK_BOT_TOKEN")]
        slack_bot_token: Option<String>,
        #[arg(long, env = "SLACK_APP_TOKEN")]
        slack_app_token: Option<String>,
        #[arg(long)]
        force: bool,
    },

    /// Manage channels
    #[command(subcommand)]
    Channels(ChannelCommands),

    /// Run the gateway (connects to stakpak serve)
    Run {
        #[arg(long, default_value = "http://127.0.0.1:4096")]
        url: String,
        #[arg(long, env = "STAKPAK_GATEWAY_TOKEN")]
        token: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        store: Option<PathBuf>,
        #[arg(long, default_value = "127.0.0.1:4097")]
        bind: String,
    },
}

#[derive(Subcommand, PartialEq, Debug)]
pub enum ChannelCommands {
    Add {
        #[arg(long)]
        channel: Option<String>,
        #[arg(long)]
        token: Option<String>,
    },
    Remove {
        #[arg(long)]
        channel: String,
    },
    Test,
    List,
}

impl GatewayCommands {
    pub async fn run(self, _config: AppConfig) -> Result<(), String> {
        match self {
            GatewayCommands::Init {
                telegram_token,
                discord_token,
                slack_bot_token,
                slack_app_token,
                force,
            } => {
                handle_init(
                    telegram_token,
                    discord_token,
                    slack_bot_token,
                    slack_app_token,
                    force,
                )
                .await
            }
            GatewayCommands::Channels(command) => handle_channels(command).await,
            GatewayCommands::Run {
                url,
                token,
                config,
                store,
                bind,
            } => handle_run(url, token, config, store, bind).await,
        }
    }
}

async fn handle_init(
    telegram_token: Option<String>,
    discord_token: Option<String>,
    slack_bot_token: Option<String>,
    slack_app_token: Option<String>,
    force: bool,
) -> Result<(), String> {
    let path = default_gateway_config_path();
    if path.exists() && !force {
        return Err(format!(
            "Gateway config already exists at {}. Use --force to overwrite.",
            path.display()
        ));
    }

    let mut config = GatewayConfig::default();

    let non_interactive = telegram_token.is_some()
        || discord_token.is_some()
        || slack_bot_token.is_some()
        || slack_app_token.is_some();

    let telegram_token = if non_interactive {
        telegram_token
    } else {
        prompt_optional("Telegram bot token (leave empty to skip): ")?
    }
    .and_then(clean_token);

    let discord_token = if non_interactive {
        discord_token
    } else {
        prompt_optional("Discord bot token (leave empty to skip): ")?
    }
    .and_then(clean_token);

    let slack_bot_token = if non_interactive {
        slack_bot_token
    } else {
        prompt_optional("Slack bot token (xoxb-, leave empty to skip): ")?
    }
    .and_then(clean_token);

    let slack_app_token = if non_interactive {
        slack_app_token
    } else if slack_bot_token.is_some() {
        prompt_optional("Slack app token (xapp-, required when bot token is set): ")?
    } else {
        None
    }
    .and_then(clean_token);

    if let Some(token) = telegram_token {
        config.channels.telegram = Some(stakpak_gateway::config::TelegramConfig {
            token,
            require_mention: false,
        });
    }

    if let Some(token) = discord_token {
        config.channels.discord = Some(stakpak_gateway::config::DiscordConfig {
            token,
            guilds: Vec::new(),
        });
    }

    match (slack_bot_token, slack_app_token) {
        (Some(bot_token), Some(app_token)) => {
            config.channels.slack = Some(stakpak_gateway::config::SlackConfig {
                bot_token,
                app_token,
            });
        }
        (None, None) => {}
        _ => {
            return Err(
                "Slack setup requires both --slack-bot-token and --slack-app-token".to_string(),
            );
        }
    }

    if !config.has_channels() {
        return Err("No channels configured. Provide at least one channel token.".to_string());
    }

    config
        .save(Some(&path))
        .map_err(|error| format!("Failed to save gateway config: {error}"))?;

    println!("✓ Gateway config saved to {}", path.display());
    Ok(())
}

async fn handle_channels(command: ChannelCommands) -> Result<(), String> {
    match command {
        ChannelCommands::List => {
            let config = GatewayConfig::load_default(&GatewayCliFlags::default())
                .map_err(|error| format!("Failed to load gateway config: {error}"))?;

            println!("CHANNEL    STATUS");
            println!(
                "telegram   {}",
                configured_text(config.channels.telegram.is_some())
            );
            println!(
                "discord    {}",
                configured_text(config.channels.discord.is_some())
            );
            println!(
                "slack      {}",
                configured_text(config.channels.slack.is_some())
            );

            Ok(())
        }
        ChannelCommands::Test => {
            let config = GatewayConfig::load_default(&GatewayCliFlags::default())
                .map_err(|error| format!("Failed to load gateway config: {error}"))?;
            let channels = build_channels(&config)
                .map_err(|error| format!("Failed to build channels: {error}"))?;

            if channels.is_empty() {
                return Err("No channels configured".to_string());
            }

            for channel in channels.values() {
                match channel.test().await {
                    Ok(result) => println!(
                        "  ✓ {}: {} ({})",
                        result.channel, result.identity, result.details
                    ),
                    Err(error) => println!("  ✗ {}: {}", channel.display_name(), error),
                }
            }

            Ok(())
        }
        ChannelCommands::Add { channel, token } => {
            let mut config = GatewayConfig::load_default(&GatewayCliFlags::default())
                .map_err(|error| format!("Failed to load gateway config: {error}"))?;

            let channel_name = channel
                .or_else(|| {
                    prompt_optional("Channel to add (telegram/discord/slack): ")
                        .ok()
                        .flatten()
                })
                .unwrap_or_default();

            match channel_name.as_str() {
                "telegram" => {
                    let token = token
                        .or_else(|| prompt_optional("Telegram token: ").ok().flatten())
                        .unwrap_or_default();
                    if token.trim().is_empty() {
                        return Err("Telegram token is required".to_string());
                    }
                    config.channels.telegram = Some(stakpak_gateway::config::TelegramConfig {
                        token,
                        require_mention: false,
                    });
                }
                "discord" => {
                    let token = token
                        .or_else(|| prompt_optional("Discord token: ").ok().flatten())
                        .unwrap_or_default();
                    if token.trim().is_empty() {
                        return Err("Discord token is required".to_string());
                    }
                    config.channels.discord = Some(stakpak_gateway::config::DiscordConfig {
                        token,
                        guilds: Vec::new(),
                    });
                }
                "slack" => {
                    let bot = prompt_optional("Slack bot token: ")?.unwrap_or_default();
                    let app = prompt_optional("Slack app token: ")?.unwrap_or_default();
                    if bot.trim().is_empty() || app.trim().is_empty() {
                        return Err("Slack bot/app tokens are required".to_string());
                    }
                    config.channels.slack = Some(stakpak_gateway::config::SlackConfig {
                        bot_token: bot,
                        app_token: app,
                    });
                }
                _ => {
                    return Err("Unsupported channel. Use telegram, discord, or slack.".to_string());
                }
            }

            config
                .save(None)
                .map_err(|error| format!("Failed to save gateway config: {error}"))?;
            println!("✓ Channel added");
            Ok(())
        }
        ChannelCommands::Remove { channel } => {
            let mut config = GatewayConfig::load_default(&GatewayCliFlags::default())
                .map_err(|error| format!("Failed to load gateway config: {error}"))?;

            match channel.as_str() {
                "telegram" => config.channels.telegram = None,
                "discord" => config.channels.discord = None,
                "slack" => config.channels.slack = None,
                _ => {
                    return Err("Unsupported channel. Use telegram, discord, or slack.".to_string());
                }
            }

            config
                .save(None)
                .map_err(|error| format!("Failed to save gateway config: {error}"))?;
            println!("✓ Channel removed");
            Ok(())
        }
    }
}

async fn handle_run(
    url: String,
    token: Option<String>,
    config_path: Option<PathBuf>,
    store: Option<PathBuf>,
    bind: String,
) -> Result<(), String> {
    let cli = GatewayCliFlags {
        url: Some(url),
        token,
        store,
        ..Default::default()
    };

    let config = GatewayConfig::load(config_path.as_deref(), &cli)
        .map_err(|error| format!("Failed to load gateway config: {error}"))?;

    let gateway = Gateway::new(config)
        .await
        .map_err(|error| format!("Failed to initialize gateway: {error}"))?;

    gateway
        .health()
        .await
        .map_err(|error| format!("Cannot connect to stakpak serve: {error}"))?;

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|error| format!("Failed to bind gateway API on {bind}: {error}"))?;

    let api = axum::Router::new().nest("/v1/gateway", gateway.api_router());
    let cancel = CancellationToken::new();

    println!("Gateway runtime started");
    println!("Gateway API: http://{bind}/v1/gateway/status");

    tokio::select! {
        result = gateway.run(cancel.clone()) => {
            if let Err(error) = result {
                return Err(format!("Gateway runtime error: {error}"));
            }
        }
        result = axum::serve(listener, api) => {
            cancel.cancel();
            if let Err(error) = result {
                return Err(format!("Gateway API error: {error}"));
            }
        }
        _ = tokio::signal::ctrl_c() => {
            cancel.cancel();
        }
    }

    Ok(())
}

fn configured_text(configured: bool) -> &'static str {
    if configured {
        "configured"
    } else {
        "not configured"
    }
}

fn clean_token(value: String) -> Option<String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn prompt_optional(prompt: &str) -> Result<Option<String>, String> {
    print!("{prompt}");
    io::stdout()
        .flush()
        .map_err(|error| format!("Failed to flush stdout: {error}"))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|error| format!("Failed to read input: {error}"))?;

    let trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}
