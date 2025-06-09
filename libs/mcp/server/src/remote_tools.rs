use rmcp::{
    Error as McpError, RoleServer, ServerHandler, model::*, schemars, service::RequestContext, tool,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use stakpak_api::models::SimpleDocument;
use stakpak_api::{Client, ClientConfig, GenerationResult, ToolsCallParams};

use std::fs;
use std::io::Write;
use std::path::Path;
use tracing::{error, warn};

use crate::secret_manager::SecretManager;
use crate::tool_descriptions::*;

/// Remote tools that require API access
#[derive(Clone)]
pub struct RemoteTools {
    api_config: ClientConfig,
    secret_manager: SecretManager,
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

impl std::fmt::Display for Provisioner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provisioner::Terraform => write!(f, "Terraform"),
            Provisioner::Kubernetes => write!(f, "Kubernetes"),
            Provisioner::Dockerfile => write!(f, "Dockerfile"),
            Provisioner::GithubActions => write!(f, "GithubActions"),
            Provisioner::None => write!(f, "None"),
        }
    }
}

#[tool(tool_box)]
impl RemoteTools {
    pub fn new(api_config: ClientConfig, redact_secrets: bool) -> Self {
        Self {
            api_config,
            secret_manager: SecretManager::new(redact_secrets),
        }
    }

    #[tool(description = GENERATE_CODE_DESCRIPTION)]
    pub async fn generate_code(
        &self,
        #[tool(param)]
        #[schemars(description = GENERATE_PROMPT_PARAM_DESCRIPTION)]
        prompt: String,
        #[tool(param)]
        #[schemars(description = PROVISIONER_PARAM_DESCRIPTION)]
        provisioner: Provisioner,
        #[tool(param)]
        #[schemars(description = SAVE_FILES_PARAM_DESCRIPTION)]
        save_files: Option<bool>,
        #[tool(param)]
        #[schemars(description = CONTEXT_PARAM_DESCRIPTION)]
        context: Option<Vec<String>>,
    ) -> Result<CallToolResult, McpError> {
        let client = Client::new(&self.api_config).map_err(|e| {
            error!("Failed to create client: {}", e);
            McpError::internal_error(
                "Failed to create client",
                Some(json!({ "error": e.to_string() })),
            )
        })?;

        let output_format = if save_files.unwrap_or(false) {
            "json"
        } else {
            "markdown"
        };

        // Convert context paths to Vec<Document>
        let context_documents = if let Some(context_paths) = context {
            context_paths
                .into_iter()
                .map(|path| {
                    let uri = format!("file://{}", path);
                    match std::fs::read_to_string(&path) {
                        Ok(content) => {
                            // Redact secrets in the file content
                            let redacted_content = self
                                .secret_manager
                                .redact_and_store_secrets(&content, Some(&path));
                            SimpleDocument {
                                uri,
                                content: redacted_content,
                            }
                        }
                        Err(e) => {
                            warn!("Failed to read context file {}: {}", path, e);
                            // Add empty document with error message
                            SimpleDocument {
                                uri,
                                content: format!("Error reading file: {}", e),
                            }
                        }
                    }
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let response = match client
            .call_mcp_tool(&ToolsCallParams {
                name: "generate_code".to_string(),
                arguments: json!({
                    "prompt": prompt,
                    "provisioner": provisioner.to_string(),
                    "context": context_documents,
                    "output_format": output_format,
                }),
            })
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("GENERATE_CODE_ERROR"),
                    Content::text(format!("Failed to generate code: {}", e)),
                ]));
            }
        };

        if save_files.unwrap_or(false) {
            let mut result_report = String::new();

            let response_text = response
                .iter()
                .map(|r| {
                    if let Some(RawTextContent { text }) = r.as_text() {
                        text.clone()
                    } else {
                        "".to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("");

            let generation_result: GenerationResult = serde_json::from_str(&response_text)
                .map_err(|e| {
                    error!("Failed to parse generation result: {}", e);
                    McpError::internal_error(
                        "Failed to parse generation result",
                        Some(json!({ "error": e.to_string() })),
                    )
                })?;

            let mut new_files: Vec<String> = Vec::new();
            let mut failed_edits = Vec::new();

            for edit in generation_result.edits.unwrap_or_default() {
                let file_path = Path::new(
                    edit.document_uri
                        .strip_prefix("file:///")
                        .unwrap_or(&edit.document_uri),
                );

                // Create parent directories if they don't exist
                if let Some(parent) = file_path.parent() {
                    if !parent.exists() {
                        if let Err(e) = fs::create_dir_all(parent) {
                            error!("Failed to create directory {}: {}", parent.display(), e);
                            failed_edits.push(format!(
                                "Failed to create directory {} for file {}: {}\nEdit content:\n{}",
                                parent.display(),
                                file_path.display(),
                                e,
                                edit
                            ));
                            continue;
                        }
                    }
                }

                // Check if file exists, if not create it
                if !file_path.exists() {
                    match fs::File::create(file_path) {
                        Ok(_) => {
                            new_files.push(file_path.to_str().unwrap_or_default().to_string());
                        }
                        Err(e) => {
                            error!("Failed to create file {}: {}", file_path.display(), e);
                            failed_edits.push(format!(
                                "Failed to create file {}: {}\nEdit content:\n{}",
                                file_path.display(),
                                e,
                                edit
                            ));
                            continue;
                        }
                    }
                }

                let redacted_edit = self
                    .secret_manager
                    .redact_and_store_secrets(&edit.to_string(), file_path.to_str());

                if edit.old_str.is_empty() {
                    // This is an addition to a file (appending content)
                    match fs::OpenOptions::new().append(true).open(file_path) {
                        Ok(mut file) => {
                            if let Err(e) = file.write_all(edit.new_str.as_bytes()) {
                                error!("Failed to append to file {}: {}", file_path.display(), e);
                                failed_edits.push(format!(
                                    "Failed to append content to file {}: {}\nEdit content:\n{}",
                                    file_path.display(),
                                    e,
                                    redacted_edit
                                ));
                                continue;
                            }
                            result_report.push_str(&format!("{}\n\n", redacted_edit));
                        }
                        Err(e) => {
                            error!(
                                "Failed to open file for appending {}: {}",
                                file_path.display(),
                                e
                            );
                            failed_edits.push(format!(
                                "Failed to open file {} for appending: {}\nEdit content:\n{}",
                                file_path.display(),
                                e,
                                redacted_edit
                            ));
                            continue;
                        }
                    }
                } else {
                    // This is a modification to a file (replacing content)
                    // Read the current file content
                    let current_content = match fs::read_to_string(file_path) {
                        Ok(content) => content,
                        Err(e) => {
                            error!("Failed to read file {}: {}", file_path.display(), e);
                            failed_edits.push(format!(
                                "Failed to read file {} for content replacement: {}\nEdit content:\n{}",
                                file_path.display(),
                                e,
                                edit
                            ));
                            continue;
                        }
                    };

                    // Verify that the file contains the old string
                    if !current_content.contains(&edit.old_str) {
                        error!(
                            "Search string not found in file {}, skipping edit: \n{}",
                            file_path.display(),
                            edit
                        );
                        failed_edits.push(format!(
                            "Search string not found in file {} - the file content may have changed or the search string is incorrect.\nEdit content:\n{}",
                            file_path.display(),
                            edit
                        ));
                        continue;
                    }

                    // Replace old content with new content
                    let updated_content = current_content.replace(&edit.old_str, &edit.new_str);
                    match fs::write(file_path, updated_content) {
                        Ok(_) => {
                            result_report.push_str(&format!("{}\n\n", redacted_edit));
                        }
                        Err(e) => {
                            error!("Failed to write to file {}: {}", file_path.display(), e);
                            failed_edits.push(format!(
                                "Failed to write updated content to file {}: {}\nEdit content:\n{}",
                                file_path.display(),
                                e,
                                redacted_edit
                            ));
                            continue;
                        }
                    }
                }
            }

            // Build the final result report
            let mut final_report = String::new();

            if !new_files.is_empty() {
                final_report.push_str(&format!("Created files: {}\n\n", new_files.join(", ")));
            }

            if !result_report.is_empty() {
                final_report.push_str("Successfully applied edits:\n");
                final_report.push_str(&result_report);
            }

            if !failed_edits.is_empty() {
                final_report.push_str("\n❌ Failed Edits:\n");
                for (i, failed_edit) in failed_edits.iter().enumerate() {
                    final_report.push_str(&format!("{}. {}\n", i + 1, failed_edit));
                }
                final_report.push_str("\nPlease review the failed edits above and take appropriate action to resolve the issues.\n");
            }

            Ok(CallToolResult::success(vec![Content::text(final_report)]))
        } else {
            Ok(CallToolResult::success(response))
        }
    }

    #[tool(description = SMART_SEARCH_CODE_DESCRIPTION)]
    pub async fn smart_search_code(
        &self,
        #[tool(param)]
        #[schemars(description = SEARCH_QUERY_PARAM_DESCRIPTION)]
        query: String,
        #[tool(param)]
        #[schemars(description = SEARCH_LIMIT_PARAM_DESCRIPTION)]
        limit: Option<u32>,
    ) -> Result<CallToolResult, McpError> {
        let client = Client::new(&self.api_config).map_err(|e| {
            error!("Failed to create client: {}", e);
            McpError::internal_error(
                "Failed to create client",
                Some(json!({ "error": e.to_string() })),
            )
        })?;

        let response = match client
            .call_mcp_tool(&ToolsCallParams {
                name: "smart_search_code".to_string(),
                arguments: json!({
                    "query": query,
                    "limit": limit,
                }),
            })
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("SMART_SEARCH_CODE_ERROR"),
                    Content::text(format!("Failed to search for code: {}", e)),
                ]));
            }
        };

        Ok(CallToolResult::success(response))
    }
}

#[tool(tool_box)]
impl ServerHandler for RemoteTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "This server provides remote tools for code generation and smart search using Stakpak API.".to_string(),
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
