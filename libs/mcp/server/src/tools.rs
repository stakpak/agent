use rmcp::{
    Error as McpError, RoleServer, ServerHandler, model::*, schemars, service::RequestContext, tool,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use stakpak_api::{Client, ClientConfig, GenerateCodeInput, models::ProvisionerType};
use std::process::Command;
use tracing::error;

#[derive(Clone)]
pub struct Tools {
    api_config: ClientConfig,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone, JsonSchema)]
pub enum Provisioner {
    #[serde(rename = "Terraform")]
    Terraform,
    #[serde(rename = "Kubernetes")]
    Kubernetes,
    #[serde(rename = "Dockerfile")]
    Dockerfile,
    #[serde(rename = "GithubActions")]
    GithubActions,
    #[serde(rename = "None")]
    None,
}

impl From<Provisioner> for ProvisionerType {
    fn from(provisioner: Provisioner) -> Self {
        match provisioner {
            Provisioner::Terraform => ProvisionerType::Terraform,
            Provisioner::Kubernetes => ProvisionerType::Kubernetes,
            Provisioner::Dockerfile => ProvisionerType::Dockerfile,
            Provisioner::GithubActions => ProvisionerType::GithubActions,
            Provisioner::None => ProvisionerType::None,
        }
    }
}

#[tool(tool_box)]
impl Tools {
    pub fn new(api_config: ClientConfig) -> Self {
        Self { api_config }
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

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let mut result = String::new();
        if exit_code != 0 {
            result.push_str(&format!("Command exited with code {}\n", exit_code));
        }
        // print stderr first, some commands show warnings in stderr
        if !stderr.is_empty() {
            let stderr = clip_output(&stderr);
            result.push_str(&stderr);
        }
        if !stdout.is_empty() {
            let stdout = clip_output(&stdout);
            result.push_str(&stdout);
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "A tool used to generate code using a given prompt and provisioner")]
    async fn generate_code(
        &self,
        #[tool(param)]
        #[schemars(description = "The prompt to use to generate the code")]
        prompt: String,
        #[tool(param)]
        #[schemars(description = "The provisioner to use to generate the code")]
        provisioner: Provisioner,
    ) -> Result<CallToolResult, McpError> {
        let client = Client::new(&self.api_config).map_err(|e| {
            error!("Failed to create client: {}", e);
            McpError::internal_error(
                "Failed to create client",
                Some(json!({ "error": e.to_string() })),
            )
        })?;

        let response = client
            .generate_code(&GenerateCodeInput {
                prompt,
                provisioner: provisioner.into(),
                resolve_validation_errors: true,
                stream: false,
            })
            .await
            .map_err(|e| {
                error!("Failed to generate code: {}", e);
                McpError::internal_error(
                    "Failed to generate code",
                    Some(json!({ "error": e.to_string() })),
                )
            })?;

        fn block_to_markdown(block: &stakpak_api::models::Block) -> String {
            format!(
                "### `{}`\n**Location:** `{}`\n```{lang}\n{code}\n```\n",
                block.name.as_deref().unwrap_or("Unnamed Block"),
                block.get_uri(),
                lang = block.language,
                code = block.code
            )
        }

        let result = response.result;

        let created_md = if !result.created_blocks.is_empty() {
            let blocks = result
                .created_blocks
                .iter()
                .map(block_to_markdown)
                .collect::<Vec<_>>()
                .join("\n");
            format!("## Created Blocks\n{}", blocks)
        } else {
            String::new()
        };
        let modified_md = if !result.modified_blocks.is_empty() {
            let blocks = result
                .modified_blocks
                .iter()
                .map(block_to_markdown)
                .collect::<Vec<_>>()
                .join("\n");
            format!("## Modified Blocks\n{}", blocks)
        } else {
            String::new()
        };
        let removed_md = if !result.removed_blocks.is_empty() {
            let blocks = result
                .removed_blocks
                .iter()
                .map(block_to_markdown)
                .collect::<Vec<_>>()
                .join("\n");
            format!("## Removed Blocks\n{}", blocks)
        } else {
            String::new()
        };

        let markdown = format!("{}\n{}\n{}", created_md, modified_md, removed_md);

        Ok(CallToolResult::success(vec![Content::text(markdown)]))
    }

    //TODO: Add after adding widget for file reading
    // #[tool(
    //     description = "A system command execution tool that allows running shell commands with full system access."
    // )]
    // fn read_file(
    //     &self,
    //     #[tool(param)]
    //     #[schemars(description = "The path to the file to read")]
    //     path: String,
    // ) -> Result<CallToolResult, McpError> {
    //     let path_clone = path.clone();
    //     let content = fs::read_to_string(path).map_err(|e| {
    //         error!("Failed to read file: {}", e);
    //         McpError::internal_error(
    //             "Failed to read file",
    //             Some(json!({ "path": path_clone, "error": e.to_string() })),
    //         )
    //     })?;

    //     Ok(CallToolResult::success(vec![Content::text(content)]))
    // }
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

pub fn clip_output(output: &str) -> String {
    const MAX_OUTPUT_LENGTH: usize = 4000;
    // Truncate long output
    if output.len() > MAX_OUTPUT_LENGTH {
        let offset = MAX_OUTPUT_LENGTH / 2;
        let start = output
            .char_indices()
            .nth(offset)
            .map(|(i, _)| i)
            .unwrap_or(output.len());
        let end = output
            .char_indices()
            .rev()
            .nth(offset)
            .map(|(i, _)| i)
            .unwrap_or(0);

        return format!("{}\n[clipped]\n{}", &output[..start], &output[end..]);
    }

    output.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_output_empty_string() {
        let output = "";
        assert_eq!(clip_output(output), "");
    }

    #[test]
    fn test_clip_output_short_string() {
        let output = "This is a short string that should not be clipped.";
        assert_eq!(clip_output(output), output);
    }

    #[test]
    fn test_clip_output_exact_length() {
        // Create a string with exactly MAX_OUTPUT_LENGTH characters
        let output = "a".repeat(4000);
        assert_eq!(clip_output(&output), output);
    }

    #[test]
    fn test_clip_output_long_string() {
        // Create a string longer than MAX_OUTPUT_LENGTH
        let output = "a".repeat(6000);
        let result = clip_output(&output);

        // Check that result has the expected format with [clipped] marker
        assert!(result.contains("[clipped]"));

        // Check the total length is as expected (2000 + 2000 + length of "\n[clipped]\n")
        let expected_length = 2000 + 2001 + "\n[clipped]\n".len();
        assert_eq!(result.len(), expected_length);
    }

    #[test]
    fn test_clip_output_unicode_characters() {
        // Create a string with unicode characters that's longer than MAX_OUTPUT_LENGTH
        // Using characters like emoji that take more than one byte
        let emoji_repeat = "😀🌍🚀".repeat(1500); // Each emoji is multiple bytes
        let result = clip_output(&emoji_repeat);

        assert!(result.contains("[clipped]"));

        // Verify the string was properly split on character boundaries
        // by checking that we don't have any invalid UTF-8 sequences
        assert!(result.chars().all(|c| c.is_ascii() || c.len_utf8() > 1));
    }
}
