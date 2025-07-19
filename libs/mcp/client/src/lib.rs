use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use local::{LocalClientHandler, local_client};
use rmcp::{
    RoleClient,
    model::{CallToolRequestParam, ClientRequest, Request, Tool},
    service::{PeerRequestOptions, RequestHandle, RunningService},
};
use stakpak_shared::cert_utils::CertificateChain;
use stakpak_shared::models::integrations::openai::ToolCallResultProgress;
use tokio::sync::mpsc::Sender;

pub mod local;

pub struct ClientManager {
    clients: HashMap<String, RunningService<RoleClient, LocalClientHandler>>,
}

impl ClientManager {
    pub async fn new(
        local_server_host: String,
        progress_tx: Option<Sender<ToolCallResultProgress>>,
        certificate_chain: Arc<Option<CertificateChain>>,
    ) -> Result<Self> {
        let client1 = local_client(local_server_host, progress_tx, certificate_chain).await?;
        Ok(Self {
            clients: HashMap::from([("local".to_string(), client1)]),
        })
    }

    pub async fn get_client(
        &self,
        client_name: &str,
    ) -> Result<&RunningService<RoleClient, LocalClientHandler>> {
        #[allow(clippy::unwrap_used)]
        let client = self.clients.get(client_name).unwrap();
        Ok(client)
    }

    pub async fn get_clients(
        &self,
    ) -> Result<Vec<&RunningService<RoleClient, LocalClientHandler>>> {
        let clients = self.clients.values().collect();
        Ok(clients)
    }

    pub async fn get_tools(&self) -> Result<HashMap<String, Vec<Tool>>> {
        let tools =
            futures::future::try_join_all(self.clients.iter().map(|(name, client)| async move {
                let tools = client.list_tools(Default::default()).await?;
                Ok::<_, anyhow::Error>((name.clone(), tools))
            }))
            .await?;
        let tools = tools
            .into_iter()
            .map(|(name, tools)| (name, tools.tools))
            .collect();
        Ok(tools)
    }

    pub async fn call_tool(
        &self,
        client_name: &str,
        params: CallToolRequestParam,
    ) -> Result<RequestHandle<RoleClient>, String> {
        #[allow(clippy::unwrap_used)]
        let client = self.clients.get(client_name).unwrap();
        client
            .send_cancellable_request(
                ClientRequest::CallToolRequest(Request::new(params)),
                PeerRequestOptions::no_options(),
            )
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn close_clients(&mut self) -> Result<()> {
        for client in self.clients.drain().map(|(_, client)| client) {
            client.cancel().await?;
        }
        Ok(())
    }
}
