use std::{collections::HashMap, sync::Arc, time::Instant};

use anyhow::{Result, anyhow};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::{
    api::{GatewayApiState, router as api_router},
    channels::{Channel, discord::DiscordChannel, slack::SlackChannel, telegram::TelegramChannel},
    client::StakpakClient,
    config::GatewayConfig,
    dispatcher::Dispatcher,
    store::GatewayStore,
};

pub struct Gateway {
    config: GatewayConfig,
    store: Arc<GatewayStore>,
    channels: HashMap<String, Arc<dyn Channel>>,
    dispatcher: Arc<Dispatcher>,
    api_state: Arc<GatewayApiState>,
}

impl Gateway {
    pub async fn new(config: GatewayConfig) -> Result<Self> {
        config.validate()?;

        let store = Arc::new(GatewayStore::open(&config.gateway.store_path).await?);
        let channels = build_channels(&config)?;

        if channels.is_empty() {
            return Err(anyhow!("gateway has no enabled channels"));
        }

        let client = StakpakClient::new(config.server.url.clone(), config.server.token.clone());

        let dispatcher = Arc::new(Dispatcher::new(
            client,
            channels.clone(),
            store.clone(),
            config.router_config(),
            config.gateway.model.clone(),
            config.gateway.approval_mode.clone(),
            config.gateway.approval_allowlist.clone(),
            config.gateway.title_template.clone(),
        ));

        let api_state = Arc::new(GatewayApiState {
            channels: channels.clone(),
            store: store.clone(),
            started_at: Instant::now(),
            delivery_context_ttl_hours: config.gateway.delivery_context_ttl_hours,
            auth_token: if config.server.token.trim().is_empty() {
                None
            } else {
                Some(config.server.token.clone())
            },
        });

        Ok(Self {
            config,
            store,
            channels,
            dispatcher,
            api_state,
        })
    }

    pub fn api_router(&self) -> axum::Router {
        api_router(self.api_state.clone())
    }

    pub fn channels(&self) -> &HashMap<String, Arc<dyn Channel>> {
        &self.channels
    }

    pub async fn run(&self, cancel: CancellationToken) -> Result<()> {
        let (inbound_tx, inbound_rx) = mpsc::channel(512);

        let runtime_cancel = CancellationToken::new();

        let mut channel_tasks = Vec::new();
        for channel in self.channels.values() {
            let channel = channel.clone();
            let tx = inbound_tx.clone();
            let task_cancel = runtime_cancel.child_token();

            channel_tasks.push(tokio::spawn(async move {
                if let Err(error) = channel.start(tx, task_cancel).await {
                    error!(channel = %channel.id().0, error = %error, "channel listener terminated");
                }
            }));
        }

        let dispatcher_cancel = runtime_cancel.child_token();
        let dispatcher = self.dispatcher.clone();
        let dispatcher_task = tokio::spawn(async move {
            dispatcher.run(inbound_rx, dispatcher_cancel).await;
        });

        let prune_store = self.store.clone();
        let prune_after_hours = self.config.gateway.prune_after_hours;
        let prune_cancel = runtime_cancel.child_token();
        let prune_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = prune_cancel.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60 * 60)) => {
                        let max_age_ms = (prune_after_hours as i64) * 60 * 60 * 1000;
                        if let Err(error) = prune_store.prune(max_age_ms).await {
                            warn!(error = %error, "failed to prune gateway sessions");
                        }
                        if let Err(error) = prune_store.prune_delivery_contexts().await {
                            warn!(error = %error, "failed to prune gateway delivery contexts");
                        }
                    }
                }
            }
        });

        info!(channels = self.channels.len(), "gateway runtime started");

        cancel.cancelled().await;
        runtime_cancel.cancel();

        for task in channel_tasks {
            let _ = task.await;
        }

        let _ = dispatcher_task.await;
        let _ = prune_task.await;

        info!("gateway runtime stopped");

        Ok(())
    }

    pub async fn health(&self) -> Result<()> {
        let client = StakpakClient::new(
            self.config.server.url.clone(),
            self.config.server.token.clone(),
        );
        client
            .health()
            .await
            .map(|_| ())
            .map_err(|error| anyhow!(error.to_string()))
    }
}

pub fn build_channels(config: &GatewayConfig) -> Result<HashMap<String, Arc<dyn Channel>>> {
    let mut channels: HashMap<String, Arc<dyn Channel>> = HashMap::new();

    if let Some(telegram) = &config.channels.telegram {
        channels.insert(
            "telegram".to_string(),
            Arc::new(TelegramChannel::new(telegram.token.clone())),
        );
    }

    if let Some(discord) = &config.channels.discord {
        channels.insert(
            "discord".to_string(),
            Arc::new(DiscordChannel::new(discord.token.clone())),
        );
    }

    if let Some(slack) = &config.channels.slack {
        channels.insert(
            "slack".to_string(),
            Arc::new(SlackChannel::new(
                slack.bot_token.clone(),
                slack.app_token.clone(),
            )),
        );
    }

    Ok(channels)
}
