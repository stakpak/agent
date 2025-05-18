use rmcp::{
    Error as McpError, RoleServer, ServerHandler, model::*, schemars, service::RequestContext, tool,
};
use serde_json::json;
use std::{fs, process::Command};
use tracing::error;

#[derive(Clone)]
pub struct Tools {}

#[tool(tool_box)]
impl Tools {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {}
    }

    fn _create_resource_text(&self, uri: &str, name: &str) -> Resource {
        RawResource::new(uri, name.to_string()).no_annotation()
    }

    #[tool(
        description = "A system command execution tool that allows running shell commands with full system access."
    )]
    fn run_command(
        &self,
        #[tool(param)]
        #[schemars(description = "The shell command to execute")]
        command: String,
        #[tool(param)]
        #[schemars(description = "Optional working directory for command execution")]
        work_dir: Option<String>,
    ) -> Result<CallToolResult, McpError> {
        let command_clone = command.clone();
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(work_dir.unwrap_or(".".to_string()))
            .output()
            .map_err(|e| {
                error!("Failed to run command: {}", e);
                McpError::internal_error(
                    "Failed to run command",
                    Some(json!({
                        "command": command_clone,
                        "error": e.to_string()
                    })),
                )
            })?;

        if !output.stderr.is_empty() {
            error!("Command error: {}", String::from_utf8_lossy(&output.stderr));
            return Err(McpError::internal_error(
                String::from_utf8_lossy(&output.stderr).to_string(),
                Some(
                    json!({ "command": command_clone, "error": String::from_utf8_lossy(&output.stderr) }),
                ),
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(
            String::from_utf8_lossy(&output.stdout).to_string(),
        )]))
    }

    #[tool(
        description = "A system command execution tool that allows running shell commands with full system access."
    )]
    fn read_file(
        &self,
        #[tool(param)]
        #[schemars(description = "The path to the file to read")]
        path: String,
    ) -> Result<CallToolResult, McpError> {
        let path_clone = path.clone();
        let content = fs::read_to_string(path).map_err(|e| {
            error!("Failed to read file: {}", e);
            McpError::internal_error(
                "Failed to read file",
                Some(json!({ "path": path_clone, "error": e.to_string() })),
            )
        })?;

        Ok(CallToolResult::success(vec![Content::text(content)]))
    }
}
#[tool(tool_box)]
impl ServerHandler for Tools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "This server provides a tool that can run commands on the system.".to_string(),
            ),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        Ok(self.get_info())
    }
}
