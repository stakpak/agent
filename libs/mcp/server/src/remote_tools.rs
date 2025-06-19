use rmcp::{
    Error as McpError, RoleServer, ServerHandler, model::*, schemars, service::RequestContext, tool,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use stakpak_api::models::{CodeIndex, SimpleDocument};
use stakpak_api::{Client, ClientConfig, GenerationResult, ToolsCallParams};
use stakpak_shared::local_store::LocalStore;

use std::fs::{self};
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
                        .strip_prefix("file://")
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

    #[tool(description = REMOTE_CODE_SEARCH_DESCRIPTION)]
    pub async fn remote_code_search(
        &self,
        #[tool(param)]
        #[schemars(description = REMOTE_CODE_SEARCH_QUERY_PARAM_DESCRIPTION)]
        query: String,
        #[tool(param)]
        #[schemars(description = REMOTE_CODE_SEARCH_LIMIT_PARAM_DESCRIPTION)]
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
                    Content::text("REMOTE_CODE_SEARCH_ERROR"),
                    Content::text(format!("Failed to search for code: {}", e)),
                ]));
            }
        };

        Ok(CallToolResult::success(response))
    }

    #[tool(description = SEARCH_DOCS_DESCRIPTION)]
    pub async fn search_docs(
        &self,
        #[tool(param)]
        #[schemars(description = SEARCH_DOCS_KEYWORDS_PARAM_DESCRIPTION)]
        keywords: Vec<String>,
        #[tool(param)]
        #[schemars(description = SEARCH_DOCS_EXCLUDE_KEYWORDS_PARAM_DESCRIPTION)]
        exclude_keywords: Option<Vec<String>>,
        #[tool(param)]
        #[schemars(description = SEARCH_DOCS_LIMIT_PARAM_DESCRIPTION)]
        limit: Option<u32>,
    ) -> Result<CallToolResult, McpError> {
        let client = Client::new(&self.api_config).map_err(|e| {
            error!("Failed to create client: {}", e);
            McpError::internal_error(
                "Failed to create client",
                Some(json!({ "error": e.to_string() })),
            )
        })?;

        // Cap the limit to a maximum of 5
        let limit = limit.map(|l| l.min(5)).or(Some(5));

        let response = match client
            .call_mcp_tool(&ToolsCallParams {
                name: "search_docs".to_string(),
                arguments: json!({
                    "keywords": keywords,
                    "exclude_keywords": exclude_keywords,
                    "limit": limit,
                }),
            })
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("SEARCH_DOCS_ERROR"),
                    Content::text(format!("Failed to search for docs: {}", e)),
                ]));
            }
        };

        Ok(CallToolResult::success(response))
    }

    #[tool(description = LOCAL_CODE_SEARCH_DESCRIPTION)]
    pub async fn local_code_search(
        &self,
        #[tool(param)]
        #[schemars(description = LOCAL_CODE_SEARCH_KEYWORDS_PARAM_DESCRIPTION)]
        keywords: Vec<String>,
        #[tool(param)]
        #[schemars(description = LOCAL_CODE_SEARCH_LIMIT_PARAM_DESCRIPTION)]
        limit: Option<u32>,
        #[tool(param)]
        #[schemars(description = LOCAL_CODE_SEARCH_SHOW_DEPENDENCIES_PARAM_DESCRIPTION)]
        show_dependencies: Option<bool>,
    ) -> Result<CallToolResult, McpError> {
        let index_str = LocalStore::read_session_data("code_index.json").map_err(|e| {
            error!("Failed to read code index: {}", e);
            McpError::internal_error(
                "Failed to read code index",
                Some(json!({ "error": e.to_string() })),
            )
        })?;
        let index_store: CodeIndex = serde_json::from_str(&index_str).map_err(|e| {
            McpError::internal_error(
                "Failed to parse code index",
                Some(json!({ "error": e.to_string() })),
            )
        })?;

        let search_limit = limit.unwrap_or(10) as usize;
        let show_deps = show_dependencies.unwrap_or(false);
        let keywords_lower: Vec<String> = keywords.iter().map(|k| k.to_lowercase()).collect();

        // Search through blocks
        let mut matching_blocks = Vec::new();

        for block in &index_store.index.blocks {
            let mut score = 0u32;
            let mut matched_keywords = std::collections::HashSet::new();

            // For each keyword, check matches and accumulate scores
            for keyword in &keywords_lower {
                let mut keyword_matched = false;

                // Check name match
                if let Some(name) = &block.name {
                    if name.to_lowercase().contains(keyword) {
                        score += 10;
                        keyword_matched = true;
                    }
                }

                // Check type match
                if let Some(type_name) = &block.r#type {
                    if type_name.to_lowercase().contains(keyword) {
                        score += 8;
                        keyword_matched = true;
                    }
                }

                // Check kind match
                if block.kind.to_lowercase().contains(keyword) {
                    score += 6;
                    keyword_matched = true;
                }

                // Check file path match
                if block.document_uri.to_lowercase().contains(keyword) {
                    score += 4;
                    keyword_matched = true;
                }

                // Check code content match
                if block.code.to_lowercase().contains(keyword) {
                    score += 2;
                    keyword_matched = true;
                }

                if keyword_matched {
                    matched_keywords.insert(keyword.clone());
                }
            }

            // Bonus points for matching multiple keywords
            if matched_keywords.len() > 1 {
                let multi_keyword_bonus = (matched_keywords.len() - 1) as u32 * 5;
                score += multi_keyword_bonus;
            }

            if score > 0 {
                matching_blocks.push((block, score));
            }
        }

        // Sort by score descending
        matching_blocks.sort_by(|a, b| b.1.cmp(&a.1));

        // Limit results
        matching_blocks.truncate(search_limit);

        if matching_blocks.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No code blocks found matching keywords: {:?}",
                keywords
            ))]));
        }

        let mut result = String::new();
        result.push_str(&format!(
            "Found {} matching code blocks for keywords: {:?}\n\n",
            matching_blocks.len(),
            keywords
        ));

        for (i, (block, score)) in matching_blocks.iter().enumerate() {
            let file_path = block
                .document_uri
                .strip_prefix("file://")
                .unwrap_or(&block.document_uri);

            // result.push_str(&format!(
            //     "{}. {}#L{}-L{} (Score: {})\n",
            //     i + 1,
            //     block
            //         .document_uri
            //         .strip_prefix("file:///")
            //         .unwrap_or(&block.document_uri),
            //     block.start_point.row + 1,
            //     block.end_point.row + 1,
            //     score
            // ));
            result.push_str(&format!("{}. {} (Score: {})\n", i + 1, file_path, score));

            // Redact secrets in the code before displaying
            let redacted_code = self
                .secret_manager
                .redact_and_store_secrets(&block.code, Some(file_path));

            // Show code with line numbers
            let code_lines: Vec<&str> = redacted_code.lines().collect();
            // let start_line_num = block.start_point.row + 1;

            result.push_str("   Code:\n");
            result.push_str("   ```\n");
            for line in code_lines.iter() {
                // let line_num = start_line_num + i;
                // result.push_str(&format!("   {:4}: {}\n", line_num, line));
                result.push_str(&format!("   {}\n", line));
            }
            result.push_str("   ```\n");

            if show_deps {
                if !block.dependencies.is_empty() {
                    result.push_str(&format!(
                        "   Dependencies ({}):\n",
                        block.dependencies.len()
                    ));
                    for dep in &block.dependencies {
                        // skip unsatisfied dependencies because there are still bugs with
                        if !dep.satisfied {
                            continue;
                        }

                        let mut min_length = usize::MAX;
                        let mut shortest_reference = Vec::new();
                        for selector in dep.selectors.iter() {
                            for reference in selector.references.iter() {
                                if reference.len() < min_length {
                                    min_length = reference.len();
                                    shortest_reference = reference.clone();
                                }
                            }
                        }

                        result.push_str(&format!(
                            "     - {}\n",
                            shortest_reference
                                .iter()
                                .map(|s| s.to_string())
                                .collect::<Vec<String>>()
                                .join("/")
                        ));
                    }
                }

                if !block.dependents.is_empty() {
                    result.push_str(&format!("   Dependents ({}):\n", block.dependents.len()));
                    for dep in &block.dependents {
                        result.push_str(&format!("     - {}\n", dep.key));
                    }
                }
            }

            result.push('\n');
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
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
