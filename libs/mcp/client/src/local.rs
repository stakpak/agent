use anyhow::Result;
use rmcp::{
    RoleClient, ServiceExt,
    model::{ClientCapabilities, ClientInfo, Implementation},
    service::RunningService,
    transport::StreamableHttpClientTransport,
};

pub async fn local_client()
-> Result<RunningService<RoleClient, rmcp::model::InitializeRequestParam>> {
    let transport = StreamableHttpClientTransport::from_uri("http://localhost:65535/mcp");
    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "Stakpak Client".to_string(),
            version: "0.0.1".to_string(),
        },
    };

    let client: RunningService<RoleClient, rmcp::model::InitializeRequestParam> =
        client_info.serve(transport).await.inspect_err(|e| {
            tracing::error!("client error: {:?}", e);
        })?;

    client.peer_info();

    Ok(client)
}
