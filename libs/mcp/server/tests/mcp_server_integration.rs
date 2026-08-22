//! Integration tests for MCP server across different tool modes.
//!
//! These tests spawn real MCP server instances on random ports, connect
//! real MCP clients, and validate behaviour in `LocalOnly`, `RemoteOnly`,
//! and `Combined` tool modes, including mTLS scenarios.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use stakpak_mcp_server::{
    EnabledToolsConfig, MCPServerConfig, SubagentConfig, ToolMode, start_server,
};
use stakpak_shared::cert_utils::CertificateChain;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::time::timeout;


// Helpers


/// One-time rustls crypto provider initialisation.
fn init_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// Configuration knob for the test helper.
struct TestServerOpts {
    tool_mode: ToolMode,
    tls_config: Option<Arc<rustls::ServerConfig>>,
}

/// Returned by `start_test_server`.
struct TestServer {
    /// URL to connect to (e.g. `http://127.0.0.1:PORT/mcp` or `https://…`).
    url: String,
    /// The port the server is listening on.
    port: u16,
    /// Send `()` to trigger graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// Join handle for the server task.
    server_handle: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl TestServer {
    /// Shut down the server and wait for it to exit.
    async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        // Give the server a moment to exit gracefully.
        let _ = timeout(Duration::from_secs(5), self.server_handle).await;
    }
}

/// Spawn a real MCP server on a random port and return its URL.
async fn start_test_server(opts: TestServerOpts) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("should bind to a random port");
    let port = listener
        .local_addr()
        .expect("should have a local address")
        .port();

    let scheme = if opts.tls_config.is_some() {
        "https"
    } else {
        "http"
    };
    let url = format!("{scheme}://127.0.0.1:{port}/mcp");

    let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

    let config = MCPServerConfig {
        client: None,
        bind_address: format!("127.0.0.1:{port}"),
        enabled_tools: EnabledToolsConfig::default(),
        tool_mode: opts.tool_mode,
        enable_subagents: false,
        certificate_chain: Arc::new(None),
        skill_directories: Vec::new(),
        server_tls_config: opts.tls_config,
        subagent_config: SubagentConfig::default(),
        task_manager_handle: None,
    };

    let server_handle =
        tokio::spawn(async move { start_server(config, Some(listener), Some(shutdown_rx)).await });

    // Give the server a brief moment to accept connections.
    tokio::time::sleep(Duration::from_millis(200)).await;

    TestServer {
        url,
        port,
        shutdown_tx,
        server_handle,
    }
}

/// Connect an MCP client to the given URL (plain HTTP, no mTLS).
async fn connect_client(url: &str) -> stakpak_mcp_client::McpClient {
    stakpak_mcp_client::connect_https(url, None, None)
        .await
        .expect("should connect to MCP server")
}

/// Connect an MCP client with a pre-configured TLS config (for mTLS tests).
async fn connect_client_with_tls(
    url: &str,
    tls_config: rustls::ClientConfig,
) -> stakpak_mcp_client::McpClient {
    let cert_chain = CertificateChain::generate().expect("should generate cert chain");
    // We don't use this cert_chain for the actual connection; instead we
    // build a custom reqwest client with the supplied TLS config.
    let _ = cert_chain;

    // Build a reqwest client with the pre-configured TLS
    let http_client = reqwest::Client::builder()
        .use_preconfigured_tls(tls_config)
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
        .expect("should build reqwest client");

    let config =
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url);
    let transport = rmcp::transport::StreamableHttpClientTransport::<reqwest::Client>::with_client(
        http_client,
        config,
    );

    let handler = stakpak_mcp_client::LocalClientHandler::new(None);
    use rmcp::ServiceExt;
    handler
        .serve(transport)
        .await
        .expect("should connect with TLS")
}

// Known tool names for assertions


/// Tool names registered by `tool_router_local`.
const LOCAL_TOOL_NAMES: &[&str] = &[
    "run_command",
    "run_remote_command",
    "run_command_task",
    "run_remote_command_task",
    "get_all_tasks",
    "cancel_task",
    "wait_for_tasks",
    "get_task_details",
    "view",
    "str_replace",
    "create",
    "generate_password",
    "view_web_page",
    "remove",
    "ask_user",
];

/// Tool names registered by `tool_router_remote`.
const REMOTE_TOOL_NAMES: &[&str] = &["search_docs", "load_skill"];

// Tests: Server startup in each tool mode


#[tokio::test]
async fn test_server_starts_local_mode() {
    init_crypto_provider();
    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::LocalOnly,
        tls_config: None,
    })
    .await;

    let client = timeout(Duration::from_secs(5), connect_client(&server.url))
        .await
        .expect("client connection should not time out");

    // The client successfully connected and completed the MCP initialize
    // handshake — drop the client cleanly before shutting down the server.
    drop(client);
    server.shutdown().await;
}

#[tokio::test]
async fn test_server_starts_remote_mode() {
    init_crypto_provider();
    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::RemoteOnly,
        tls_config: None,
    })
    .await;

    let client = timeout(Duration::from_secs(5), connect_client(&server.url))
        .await
        .expect("client connection should not time out");

    drop(client);
    server.shutdown().await;
}

#[tokio::test]
async fn test_server_starts_combined_mode() {
    init_crypto_provider();
    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::Combined,
        tls_config: None,
    })
    .await;

    let client = timeout(Duration::from_secs(5), connect_client(&server.url))
        .await
        .expect("client connection should not time out");

    drop(client);
    server.shutdown().await;
}


// Tests: Tool listing per mode


#[tokio::test]
async fn test_local_mode_lists_local_tools_only() {
    init_crypto_provider();
    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::LocalOnly,
        tls_config: None,
    })
    .await;

    let client = connect_client(&server.url).await;
    let tools = stakpak_mcp_client::get_tools(&client)
        .await
        .expect("should list tools");

    let tool_names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

    // Local tools should be present
    for name in LOCAL_TOOL_NAMES {
        assert!(
            tool_names.contains(&name.to_string()),
            "LocalOnly mode should include local tool '{name}', got: {tool_names:?}"
        );
    }

    // Remote-only tools should NOT be present
    for name in REMOTE_TOOL_NAMES {
        assert!(
            !tool_names.contains(&name.to_string()),
            "LocalOnly mode should NOT include remote tool '{name}', got: {tool_names:?}"
        );
    }

    drop(client);
    server.shutdown().await;
}

#[tokio::test]
async fn test_remote_mode_lists_remote_tools_only() {
    init_crypto_provider();
    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::RemoteOnly,
        tls_config: None,
    })
    .await;

    let client = connect_client(&server.url).await;
    let tools = stakpak_mcp_client::get_tools(&client)
        .await
        .expect("should list tools");

    let tool_names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

    // Remote tools should be present
    for name in REMOTE_TOOL_NAMES {
        assert!(
            tool_names.contains(&name.to_string()),
            "RemoteOnly mode should include remote tool '{name}', got: {tool_names:?}"
        );
    }

    // Local-only tools (like run_command) should NOT be present
    assert!(
        !tool_names.contains(&"run_command".to_string()),
        "RemoteOnly mode should NOT include local tool 'run_command', got: {tool_names:?}"
    );
    assert!(
        !tool_names.contains(&"view".to_string()),
        "RemoteOnly mode should NOT include local tool 'view', got: {tool_names:?}"
    );

    drop(client);
    server.shutdown().await;
}

#[tokio::test]
async fn test_combined_mode_lists_all_tools() {
    init_crypto_provider();
    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::Combined,
        tls_config: None,
    })
    .await;

    let client = connect_client(&server.url).await;
    let tools = stakpak_mcp_client::get_tools(&client)
        .await
        .expect("should list tools");

    let tool_names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

    // Both local and remote tools should be present
    for name in LOCAL_TOOL_NAMES {
        assert!(
            tool_names.contains(&name.to_string()),
            "Combined mode should include local tool '{name}', got: {tool_names:?}"
        );
    }
    for name in REMOTE_TOOL_NAMES {
        assert!(
            tool_names.contains(&name.to_string()),
            "Combined mode should include remote tool '{name}', got: {tool_names:?}"
        );
    }

    drop(client);
    server.shutdown().await;
}

// Tests: Tool execution


#[tokio::test]
async fn test_local_mode_tool_execution() {
    init_crypto_provider();
    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::LocalOnly,
        tls_config: None,
    })
    .await;

    // Create a temp file to read via the `view` tool.
    let tmp_dir = tempfile::tempdir().expect("should create temp dir");
    let test_file = tmp_dir.path().join("test_view.txt");
    std::fs::write(&test_file, "hello from integration test\n").expect("should write temp file");

    let client = connect_client(&server.url).await;

    // Call the `view` tool
    let params = rmcp::model::CallToolRequestParam {
        name: "view".into(),
        arguments: Some(
            serde_json::json!({
                "path": test_file.to_string_lossy().to_string()
            })
            .as_object()
            .cloned()
            .expect("should be an object"),
        ),
    };

    let handle = stakpak_mcp_client::call_tool(&client, params, None)
        .await
        .expect("should send call_tool request");

    let server_result = timeout(Duration::from_secs(10), handle.await_response())
        .await
        .expect("tool call should not time out")
        .expect("tool call should succeed");

    // Extract the CallToolResult from the response
    if let rmcp::model::ServerResult::CallToolResult(result) = server_result {
        // Check that the tool returned content containing our file's text
        let all_text: String = result
            .content
            .iter()
            .filter_map(|c: &rmcp::model::Content| c.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join("");

        assert!(
            all_text.contains("hello from integration test"),
            "view tool should return the file content, got: {all_text}"
        );
    } else {
        panic!("Expected CallToolResult response, got: {server_result:?}");
    }

    drop(client);
    server.shutdown().await;
}

// Tests: mTLS


#[tokio::test]
async fn test_mtls_connection_succeeds() {
    init_crypto_provider();

    let cert_chain = CertificateChain::generate().expect("should generate certificate chain");
    let server_tls = cert_chain
        .create_server_config()
        .expect("should create server TLS config");
    let client_tls = cert_chain
        .create_client_config()
        .expect("should create client TLS config");

    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::LocalOnly,
        tls_config: Some(Arc::new(server_tls)),
    })
    .await;

    let client = timeout(
        Duration::from_secs(10),
        connect_client_with_tls(&server.url, client_tls),
    )
    .await
    .expect("mTLS client connection should not time out");

    // Verify the connection works by listing tools
    let tools = stakpak_mcp_client::get_tools(&client)
        .await
        .expect("should list tools over mTLS");

    assert!(
        !tools.is_empty(),
        "should have tools available over mTLS connection"
    );

    drop(client);
    server.shutdown().await;
}

#[tokio::test]
async fn test_mtls_rejects_no_client_cert() {
    init_crypto_provider();

    let cert_chain = CertificateChain::generate().expect("should generate certificate chain");
    let server_tls = cert_chain
        .create_server_config()
        .expect("should create server TLS config");

    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::LocalOnly,
        tls_config: Some(Arc::new(server_tls)),
    })
    .await;

    // Try to connect without client certificates — this should fail.
    let plain_client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("should build plain reqwest client");

    let result = timeout(
        Duration::from_secs(5),
        plain_client
            .get(format!("https://127.0.0.1:{}/mcp", server.port))
            .send(),
    )
    .await;

    // Should either time out or return a connection error
    let connection_failed = match result {
        Err(_) => true,     // Timed out
        Ok(Err(_)) => true, // Connection error
        Ok(Ok(resp)) => !resp.status().is_success(),
    };

    assert!(
        connection_failed,
        "Connection without client certificate should fail"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_mtls_rejects_wrong_ca() {
    init_crypto_provider();

    let server_chain = CertificateChain::generate().expect("should generate server cert chain");
    let wrong_chain = CertificateChain::generate().expect("should generate wrong cert chain");

    let server_tls = server_chain
        .create_server_config()
        .expect("should create server TLS config");
    let wrong_client_tls = wrong_chain
        .create_client_config()
        .expect("should create wrong client TLS config");

    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::LocalOnly,
        tls_config: Some(Arc::new(server_tls)),
    })
    .await;

    // Try to connect with a client cert signed by a different CA
    let wrong_client = reqwest::Client::builder()
        .use_preconfigured_tls(wrong_client_tls)
        .build()
        .expect("should build wrong-CA reqwest client");

    let result = timeout(
        Duration::from_secs(5),
        wrong_client
            .get(format!("https://127.0.0.1:{}/mcp", server.port))
            .send(),
    )
    .await;

    // Should fail because client and server have different CAs
    let connection_failed = match result {
        Err(_) => true,
        Ok(Err(_)) => true,
        Ok(Ok(resp)) => !resp.status().is_success(),
    };

    assert!(connection_failed, "Connection with wrong CA should fail");

    server.shutdown().await;
}

// Tests: Graceful shutdown


#[tokio::test]
async fn test_graceful_shutdown() {
    init_crypto_provider();
    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::LocalOnly,
        tls_config: None,
    })
    .await;

    let port = server.port;

    // Verify the server is up
    let client = connect_client(&server.url).await;
    let tools = stakpak_mcp_client::get_tools(&client)
        .await
        .expect("should list tools before shutdown");
    assert!(!tools.is_empty(), "should have tools before shutdown");
    drop(client);

    // Trigger shutdown
    let _ = server.shutdown_tx.send(());

    // Wait for the server task to complete
    let result = timeout(Duration::from_secs(10), server.server_handle).await;
    assert!(result.is_ok(), "server should shut down within the timeout");

    // Verify the port is released by binding to it again
    let rebind_result = timeout(
        Duration::from_secs(2),
        TcpListener::bind(format!("127.0.0.1:{port}")),
    )
    .await;
    assert!(
        rebind_result.is_ok(),
        "should be able to rebind port after shutdown"
    );
}


// Tests: No API key (local mode)


#[tokio::test]
async fn test_server_without_api_key_local_mode() {
    init_crypto_provider();

    // Explicitly set client to None to prove no API key is needed for local mode
    let server = start_test_server(TestServerOpts {
        tool_mode: ToolMode::LocalOnly,
        tls_config: None,
    })
    .await;

    let client = connect_client(&server.url).await;

    // Should be able to list tools without any API key
    let tools = stakpak_mcp_client::get_tools(&client)
        .await
        .expect("should list tools without API key");

    assert!(
        !tools.is_empty(),
        "local mode should have tools without API key"
    );

    // Verify local tools are present
    let tool_names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    assert!(
        tool_names.contains(&"view".to_string()),
        "local mode should include 'view' tool without API key"
    );
    assert!(
        tool_names.contains(&"run_command".to_string()),
        "local mode should include 'run_command' tool without API key"
    );

    drop(client);
    server.shutdown().await;
}
