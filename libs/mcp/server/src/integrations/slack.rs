use crate::tool_container::ToolContainer;
use rmcp::{
    Error as McpError, handler::server::tool::Parameters, model::*, schemars, tool, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use stakpak_api::ToolsCallParams;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SlackReadMessages {
    #[schemars(description = "Slack channel identifier. Accepts channel ID (e.g., 'C12345678').")]
    pub channel: String,
    #[schemars(
        description = "Maximum number of messages to return (default: 10, max: 100). Returns most recent messages first."
    )]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SlackReadReplies {
    #[schemars(
        description = "Slack channel identifier that contains the thread (channel ID like 'C12345678')."
    )]
    pub channel: String,
    #[schemars(
        description = "The root message timestamp of the thread (Slack 'ts' value) to fetch replies for, for example '1727287045.000600'."
    )]
    pub ts: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SlackSendMessage {
    #[schemars(
        description = "Target Slack channel identifier. Accepts channel ID (e.g., 'C12345678')."
    )]
    pub channel: String,
    #[schemars(
        description = "Message text using Slack mrkdwn (Slack's Markdown-like subset).\n\
                       Supported: *bold*, _italic_, ~strikethrough~, `inline code`, ```code blocks```, \
                       > blockquotes, bullet/numbered lists, links (<https://example.com|label>), mentions, emojis.\n\
                       Not supported: HTML, Markdown tables, headings, underline, multi-column layouts. \
                       For table-like output, use aligned monospace text in a code block. Use plain text if unsure."
    )]
    pub mrkdwn_text: String,
    #[schemars(
        description = "Optional Slack thread 'ts'. When provided, posts the message as a reply in that thread; otherwise posts a new top-level message."
    )]
    pub thread_ts: Option<String>,
}

#[tool_router(router = tool_router_slack, vis = "pub")]
impl ToolContainer {
    #[tool(
        description = "Read and retrieve the contents of a Slack channel. This tool allows you to access and read messages from a Slack channel."
    )]
    pub async fn slack_read_messages(
        &self,
        Parameters(SlackReadMessages { channel, limit }): Parameters<SlackReadMessages>,
    ) -> Result<CallToolResult, McpError> {
        let client = match self.get_client() {
            Some(client) => client,
            None => {
                return Ok(CallToolResult::error(vec![
                    Content::text("CLIENT_NOT_FOUND"),
                    Content::text("Client not found"),
                ]));
            }
        };

        let response = match client
            .call_mcp_tool(&ToolsCallParams {
                name: "slack_read_messages".to_string(),
                arguments: json!({
                    "channel": channel,
                    "limit": limit,
                }),
            })
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("SLACK_READ_MESSAGES_ERROR"),
                    Content::text(format!("Failed to read Slack messages: {}", e)),
                ]));
            }
        };

        Ok(CallToolResult::success(response))
    }

    #[tool(
        description = "Read and retrieve the contents of a Slack thread. This tool allows you to access and read replies from a Slack thread."
    )]
    pub async fn slack_read_replies(
        &self,
        Parameters(SlackReadReplies { channel, ts }): Parameters<SlackReadReplies>,
    ) -> Result<CallToolResult, McpError> {
        let client = match self.get_client() {
            Some(client) => client,
            None => {
                return Ok(CallToolResult::error(vec![
                    Content::text("CLIENT_NOT_FOUND"),
                    Content::text("Client not found"),
                ]));
            }
        };

        let response = match client
            .call_mcp_tool(&ToolsCallParams {
                name: "slack_read_replies".to_string(),
                arguments: json!({
                    "channel": channel,
                    "ts": ts,
                }),
            })
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("SLACK_READ_REPLIES_ERROR"),
                    Content::text(format!("Failed to read Slack replies: {}", e)),
                ]));
            }
        };

        Ok(CallToolResult::success(response))
    }

    #[tool(
        description = "Send a message to a Slack channel. This tool allows you to send messages to a Slack channel."
    )]
    pub async fn slack_send_message(
        &self,
        Parameters(SlackSendMessage {
            channel,
            mrkdwn_text,
            thread_ts,
        }): Parameters<SlackSendMessage>,
    ) -> Result<CallToolResult, McpError> {
        let client = match self.get_client() {
            Some(client) => client,
            None => {
                return Ok(CallToolResult::error(vec![
                    Content::text("CLIENT_NOT_FOUND"),
                    Content::text("Client not found"),
                ]));
            }
        };

        let response = match client
            .call_mcp_tool(&ToolsCallParams {
                name: "slack_send_message".to_string(),
                arguments: json!({
                    "channel": channel,
                    "markdown_text": mrkdwn_text,
                    "thread_ts": thread_ts,
                }),
            })
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("SEND_SLACK_MESSAGE_ERROR"),
                    Content::text(format!("Failed to send Slack message: {}", e)),
                ]));
            }
        };

        Ok(CallToolResult::success(response))
    }
}
