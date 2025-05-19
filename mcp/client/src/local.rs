use anyhow::Result;
use rmcp::{
    RoleClient, serve_client,
    service::RunningService,
    transport::{ConfigureCommandExt, TokioChildProcess},
};

use tokio::process::Command;

pub async fn local_client() -> Result<RunningService<RoleClient, ()>> {
    let service = serve_client(
        (),
        TokioChildProcess::new(Command::new("cargo").configure(|cmd| {
            cmd.arg("run");
            cmd.arg("mcp");
        }))?,
    )
    .await
    .inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    service.peer_info();

    Ok(service)
}
