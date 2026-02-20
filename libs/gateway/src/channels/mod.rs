pub mod discord;
pub mod slack;
pub mod telegram;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::types::{ChannelId, InboundMessage, OutboundReply};

#[derive(Debug, Clone)]
pub struct ApprovalButton {
    pub label: String,
    pub callback_data: String,
    pub style: ButtonStyle,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ButtonStyle {
    Success,
    Danger,
}

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

    async fn send_with_buttons(
        &self,
        reply: OutboundReply,
        buttons: Vec<ApprovalButton>,
    ) -> Result<String> {
        let _ = buttons;
        self.send(reply).await?;
        Ok(String::new())
    }

    async fn edit_message(&self, message_id: &str, new_text: &str) -> Result<()> {
        let _ = (message_id, new_text);
        Ok(())
    }

    async fn test(&self) -> Result<ChannelTestResult>;
}

pub fn parse_approval_callback(data: &str) -> Option<(&str, &str)> {
    let rest = data.strip_prefix("a:")?;
    let (approval_id, decision) = rest.split_once(':')?;
    if approval_id.is_empty() || !matches!(decision, "allow" | "deny") {
        return None;
    }

    Some((approval_id, decision))
}

#[cfg(test)]
mod tests {
    use super::parse_approval_callback;

    #[test]
    fn parse_approval_callback_accepts_valid_payloads() {
        assert_eq!(
            parse_approval_callback("a:a3f0c92d:allow"),
            Some(("a3f0c92d", "allow"))
        );
        assert_eq!(
            parse_approval_callback("a:a3f0c92d:deny"),
            Some(("a3f0c92d", "deny"))
        );
    }

    #[test]
    fn parse_approval_callback_rejects_invalid_payloads() {
        assert_eq!(parse_approval_callback(""), None);
        assert_eq!(parse_approval_callback("a::allow"), None);
        assert_eq!(parse_approval_callback("a:a3f0c92d:maybe"), None);
        assert_eq!(parse_approval_callback("x:a3f0c92d:allow"), None);
        assert_eq!(parse_approval_callback("a:a3f0c92d"), None);
    }
}
