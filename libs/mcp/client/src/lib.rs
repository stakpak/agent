use std::collections::HashMap;

use anyhow::Result;
use rmcp::{
    RoleClient,
    model::{CallToolRequestParam, Tool},
    service::RunningService,
};

mod local;
use crate::local::local_client;

pub struct ClientManager {
    clients: HashMap<String, RunningService<RoleClient, ()>>,
}

impl ClientManager {
    pub async fn new() -> Result<Self> {
        let client1 = local_client().await?;
        Ok(Self {
            clients: HashMap::from([("local".to_string(), client1)]),
        })
    }

    pub async fn get_client(&self, client_name: &str) -> Result<&RunningService<RoleClient, ()>> {
        let client = self.clients.get(client_name).unwrap();
        Ok(client)
    }

    pub async fn get_clients(&self) -> Result<Vec<&RunningService<RoleClient, ()>>> {
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
        &mut self,
        client_name: &str,
        params: CallToolRequestParam,
    ) -> Result<()> {
        let client = self.clients.get_mut(client_name).unwrap();
        client.call_tool(params).await?;
        Ok(())
    }

    pub async fn close_clients(&mut self) -> Result<()> {
        for client in self.clients.drain().map(|(_, client)| client) {
            client.cancel().await?;
        }
        Ok(())
    }
}
