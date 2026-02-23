pub mod discord;
pub mod slack;
pub mod telegram;

use anyhow::{Result, anyhow};
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
        _reply: OutboundReply,
        _buttons: Vec<ApprovalButton>,
    ) -> Result<String> {
        Err(anyhow!(
            "channel '{}' does not support interactive approval buttons",
            self.display_name()
        ))
    }

    async fn edit_message(&self, _message_id: &str, _new_text: &str) -> Result<()> {
        Err(anyhow!(
            "channel '{}' does not support editing messages",
            self.display_name()
        ))
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
    use anyhow::Result;
    use async_trait::async_trait;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::{Channel, ChannelId, ChannelTestResult, parse_approval_callback};
    use crate::types::{ChatType, InboundMessage, OutboundReply, PeerId};

    #[derive(Clone)]
    struct DefaultBehaviorChannel {
        id: ChannelId,
    }

    impl DefaultBehaviorChannel {
        fn new() -> Self {
            Self {
                id: ChannelId("default-test".to_string()),
            }
        }
    }

    #[async_trait]
    impl Channel for DefaultBehaviorChannel {
        fn id(&self) -> &ChannelId {
            &self.id
        }

        fn display_name(&self) -> &str {
            "DefaultOnly"
        }

        async fn start(
            &self,
            _inbound_tx: mpsc::Sender<InboundMessage>,
            _cancel: CancellationToken,
        ) -> Result<()> {
            Ok(())
        }

        async fn send(&self, _reply: OutboundReply) -> Result<()> {
            Ok(())
        }

        async fn test(&self) -> Result<ChannelTestResult> {
            Ok(ChannelTestResult {
                channel: self.id.0.clone(),
                identity: "default-only".to_string(),
                details: "ok".to_string(),
            })
        }
    }

    fn outbound_reply() -> OutboundReply {
        OutboundReply {
            channel: ChannelId("default-test".to_string()),
            peer_id: PeerId("peer-1".to_string()),
            chat_type: ChatType::Direct,
            text: "hello".to_string(),
            metadata: serde_json::json!({}),
        }
    }

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

    #[tokio::test]
    async fn channel_default_send_with_buttons_returns_error() {
        let channel = DefaultBehaviorChannel::new();
        let result = channel
            .send_with_buttons(outbound_reply(), Vec::new())
            .await;
        assert!(result.is_err());
        let error = match result {
            Ok(_) => String::new(),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("does not support interactive approval buttons"));
        assert!(error.contains("DefaultOnly"));
    }

    #[tokio::test]
    async fn channel_default_edit_message_returns_error() {
        let channel = DefaultBehaviorChannel::new();
        let result = channel.edit_message("msg-1", "updated").await;
        assert!(result.is_err());
        let error = match result {
            Ok(_) => String::new(),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("does not support editing messages"));
        assert!(error.contains("DefaultOnly"));
    }
}
