#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use agent_client_protocol::{
    self as acp, Agent, Client, ClientSideConnection, ContentBlock, ContentChunk,
    InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion, RequestPermissionRequest,
    RequestPermissionResponse, SessionNotification, SessionUpdate, StopReason, TextContent,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{
    ErrorData as McpError, ServerHandler, handler::server::router::tool::ToolRouter, model::*,
    tool, tool_handler, tool_router,
};
use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Fake code returned by the MCP server - an LLM couldn't know this from memory
const FAKE_CODE: &str = "test-uuid-12345-67890";

#[tokio::test]
async fn test_acp_basic_completion() {
    let prompt = "what is 1+1";
    let mock_server = setup_mock_openai(vec![(
        prompt.to_string(), // Just match the prompt text
        include_str!("./test_data/openai_basic_response.txt"),
    )])
    .await;

    run_acp_session(
        &mock_server,
        vec![],
        |conn, session_id, updates| async move {
            let response = conn
                .prompt(PromptRequest::new(
                    session_id,
                    vec![ContentBlock::Text(TextContent::new(prompt))],
                ))
                .await
                .unwrap();

            assert_eq!(response.stop_reason, StopReason::EndTurn);
            wait_for_text(&updates, "2", Duration::from_secs(5)).await;
        },
    )
    .await;
}

#[tokio::test]
async fn test_acp_with_mcp_http_server() {
    let prompt = "Use the get_code tool and output only its result.";
    let (mcp_url, _handle) = spawn_mcp_http_server().await;

    let mock_server = setup_mock_openai(vec![
        (
            format!(r#"{prompt}\n</message>\n</history>","role":"user""#),
            include_str!("./test_data/openai_tool_call_response.txt"),
        ),
        (
            format!(r#"{FAKE_CODE}\n</result>\n</action>\n</history>","role":"user""#),
            include_str!("./test_data/openai_tool_result_response.txt"),
        ),
    ])
    .await;

    run_acp_session(
        &mock_server,
        vec![acp::McpServer::Http(acp::McpServerHttp::new(
            "lookup", &mcp_url,
        ))],
        |conn, session_id, updates| async move {
            let response = conn
                .prompt(PromptRequest::new(
                    session_id,
                    vec![ContentBlock::Text(TextContent::new(prompt))],
                ))
                .await
                .unwrap();

            assert_eq!(response.stop_reason, StopReason::EndTurn);
            wait_for_text(&updates, FAKE_CODE, Duration::from_secs(5)).await;
        },
    )
    .await;
}

async fn wait_for_text(
    updates: &Arc<Mutex<Vec<SessionNotification>>>,
    expected: &str,
    timeout: Duration,
) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let actual = extract_text(&updates.lock().unwrap());
        if actual.contains(expected) {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("Timeout waiting for text.\nExpected to contain: {expected}\nActual: {actual}");
        }
        tokio::task::yield_now().await;
    }
}

const TITLE_GENERATION_RESPONSE: &str = include_str!("./test_data/openai_session_description.json");

async fn setup_mock_openai(exchanges: Vec<(String, &'static str)>) -> MockServer {
    let mock_server = MockServer::start().await;
    let queue: VecDeque<(String, &'static str)> = exchanges.into_iter().collect();
    let queue = Arc::new(Mutex::new(queue));

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with({
            let queue = queue.clone();
            move |req: &wiremock::Request| {
                let body = String::from_utf8_lossy(&req.body);
                let is_streaming = body.contains(r#""stream":true"#);

                if !is_streaming {
                    return ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_string(TITLE_GENERATION_RESPONSE);
                }

                let (expected, response) = {
                    let mut q = queue.lock().unwrap();
                    match q.pop_front() {
                        Some(item) => item,
                        None => {
                            return ResponseTemplate::new(500)
                                .set_body_string(format!("unexpected request: {body}"));
                        }
                    }
                };

                if !body.contains(&expected) {
                    return ResponseTemplate::new(500).set_body_string(format!(
                        "expected body to contain: {expected}\nactual: {body}"
                    ));
                }

                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(response)
            }
        })
        .mount(&mock_server)
        .await;

    mock_server
}

fn extract_text(updates: &[SessionNotification]) -> String {
    updates
        .iter()
        .filter_map(|n| match &n.update {
            SessionUpdate::AgentMessageChunk(ContentChunk {
                content: ContentBlock::Text(t),
                ..
            }) => Some(t.text.clone()),
            _ => None,
        })
        .collect()
}

fn create_test_config(mock_server: &MockServer, work_dir: &Path) -> std::path::PathBuf {
    let config_path = work_dir.join("config.toml");
    let uri = mock_server.uri();
    let db_path = work_dir.join("local.db").to_string_lossy().to_string();
    let config_content = format!(
        r#"[profiles.default]
api_key = "test"
provider = "local"
smart_model = "gpt-4"
eco_model = "gpt-4"
store_path = "{db_path}"
[profiles.default.openai]
api_endpoint = "{uri}/v1/chat/completions"
api_key = "test"
[settings]
"#
    );
    std::fs::write(&config_path, config_content).unwrap();
    config_path
}

async fn spawn_stakpak_acp(config_path: &Path) -> Child {
    Command::new(&*common::STAKPAK_BINARY)
        .args(["--config", config_path.to_str().unwrap(), "acp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env(
            "RUST_LOG",
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        )
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

#[derive(Clone)]
struct TestClient {
    updates: Arc<Mutex<Vec<SessionNotification>>>,
}

#[async_trait::async_trait(?Send)]
impl Client for TestClient {
    async fn request_permission(
        &self,
        request: RequestPermissionRequest,
    ) -> acp::Result<RequestPermissionResponse> {
        // Auto-approve the first option
        let option_id = request.options.first().map(|opt| opt.option_id.clone());
        match option_id {
            Some(id) => Ok(RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(id)),
            )),
            None => Ok(RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled,
            )),
        }
    }

    async fn write_text_file(
        &self,
        _args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        Ok(acp::WriteTextFileResponse::default())
    }

    async fn read_text_file(
        &self,
        _args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        Ok(acp::ReadTextFileResponse::new(""))
    }

    async fn session_notification(&self, args: SessionNotification) -> acp::Result<()> {
        self.updates.lock().unwrap().push(args);
        Ok(())
    }

    async fn create_terminal(
        &self,
        _args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        unimplemented!()
    }

    async fn terminal_output(
        &self,
        _args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        unimplemented!()
    }

    async fn kill_terminal_command(
        &self,
        _args: acp::KillTerminalCommandRequest,
    ) -> acp::Result<acp::KillTerminalCommandResponse> {
        unimplemented!()
    }

    async fn release_terminal(
        &self,
        _args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        unimplemented!()
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        unimplemented!()
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
    }
}

async fn run_acp_session<F, Fut>(
    mock_server: &MockServer,
    mcp_servers: Vec<acp::McpServer>,
    test_fn: F,
) where
    F: FnOnce(ClientSideConnection, acp::SessionId, Arc<Mutex<Vec<SessionNotification>>>) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let work_dir = tempfile::tempdir().unwrap();
    let config_path = create_test_config(mock_server, work_dir.path());
    let mut child = spawn_stakpak_acp(&config_path).await;
    let updates = Arc::new(Mutex::new(Vec::new()));

    let outgoing = child.stdin.take().unwrap().compat_write();
    let incoming = child.stdout.take().unwrap().compat();
    let stderr = child.stderr.take().unwrap();

    // Drain stderr to prevent subprocess from blocking
    let stderr_handle = tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while lines.next_line().await.is_ok_and(|l| l.is_some()) {}
    });

    let local_set = tokio::task::LocalSet::new();
    local_set
        .run_until(async move {
            let client = TestClient {
                updates: updates.clone(),
            };
            let (conn, handle_io) = ClientSideConnection::new(client, outgoing, incoming, |fut| {
                tokio::task::spawn_local(fut);
            });
            tokio::task::spawn_local(async move {
                let _ = handle_io.await;
            });

            conn.initialize(InitializeRequest::new(ProtocolVersion::LATEST))
                .await
                .unwrap();

            let session = conn
                .new_session(NewSessionRequest::new(work_dir.path()).mcp_servers(mcp_servers))
                .await
                .unwrap();

            test_fn(conn, session.session_id, updates).await;
        })
        .await;

    // Kill the child and wait for stderr reader to finish
    drop(child);
    let _ = stderr_handle.await;
}

// MCP HTTP server with get_code tool
#[derive(Clone)]
struct Lookup {
    tool_router: ToolRouter<Lookup>,
}

#[tool_router]
impl Lookup {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Get the code")]
    fn get_code(&self) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(FAKE_CODE)]))
    }
}

#[tool_handler]
impl ServerHandler for Lookup {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Lookup server with get_code tool.".into()),
        }
    }
}

async fn spawn_mcp_http_server() -> (String, JoinHandle<()>) {
    let service = StreamableHttpService::new(
        || Ok(Lookup::new()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}/mcp");

    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    (url, handle)
}
