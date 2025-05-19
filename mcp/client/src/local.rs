use anyhow::Result;
use rmcp::{
    RoleClient, serve_client,
    service::RunningService,
    transport::{ConfigureCommandExt, TokioChildProcess},
};

use tokio::process::Command;

pub async fn local_client() -> Result<RunningService<RoleClient, ()>> {
    // Run in dev mode
    let process = match cfg!(debug_assertions) {
        true => TokioChildProcess::new(Command::new("cargo").configure(|cmd| {
            cmd.arg("run");
            cmd.arg("mcp");
        })),
        false => TokioChildProcess::new(Command::new("stakpak").configure(|cmd| {
            cmd.arg("mcp");
        })),
    }?;

    let service = serve_client((), process).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    service.peer_info();

    Ok(service)
}
