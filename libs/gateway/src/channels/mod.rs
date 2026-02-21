pub mod discord;
pub mod slack;
pub mod telegram;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::types::{ChannelId, InboundMessage, OutboundReply};

#[derive(Debug, Clone, Default)]
pub struct DeliveryReceipt {
    pub message_id: Option<String>,
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChannelTestResult {
    pub channel: String,
    pub identity: String,
    pub details: String,
}

#[async_trait]
pub trait Channel: Send + Sync + 'static {
    fn id(&self) -> &ChannelId;

    fn display_name(&self) -> &str;

    async fn start(
        &self,
        inbound_tx: mpsc::Sender<InboundMessage>,
        cancel: CancellationToken,
    ) -> Result<()>;

    async fn send(&self, reply: OutboundReply) -> Result<()>;

    async fn send_with_receipt(&self, reply: OutboundReply) -> Result<DeliveryReceipt> {
        self.send(reply).await?;
        Ok(DeliveryReceipt::default())
    }

    async fn test(&self) -> Result<ChannelTestResult>;
}
