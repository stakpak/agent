use crate::tool_container::ToolContainer;
use chrono::{DateTime, Utc};
use rmcp::{
    ErrorData as McpError, handler::server::wrapper::Parameters, model::*, schemars, tool,
    tool_router,
};
use serde::Deserialize;
use serde_json::json;
use stakpak_api::ToolsCallParams;
use stakpak_api::models::CodeIndex;
use stakpak_shared::local_store::LocalStore;
use stakpak_shared::models::indexing::IndexingStatus;
use tracing::error;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GenerateCodeRequest {
    #[schemars(
        description = "Prompt to use to generate code, this should be as detailed as possible. Make sure to specify the paths of the files to be created or modified if you want to save changes to the filesystem."
    )]
    pub prompt: String,
    #[schemars(
        description = "Whether to save the generated files to the filesystem (default: false)"
    )]
    pub save_files: Option<bool>,
    #[schemars(
        description = "Optional list of file paths to include as context for the generation. CRITICAL: When generating code in multiple steps (breaking down large projects), always include previously generated files from earlier steps to ensure consistent references, imports, and overall project coherence. Add any files you want to edit, or that you want to use as context for the generation (default: empty)"
    )]
    pub context: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchDocsRequest {
    #[schemars(
        description = "Space-separated keywords (e.g., 'kubernetes ingress nginx ssl'). Use hyphens for compound terms like 'cloud-native'."
    )]
    pub keywords: String,
    #[schemars(
        description = "Space-separated keywords to exclude from the search results (e.g., 'deprecated legacy'). This is useful for filtering out documentation sources that are not relevant to the query."
    )]
    pub exclude_keywords: Option<String>,
    #[schemars(description = "The maximum number of results to return (default: 5, max: 5)")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchMemoryRequest {
    #[schemars(
        description = "Space-separated keywords to search for in your memory (e.g., 'kubernetes deployment config'). Searches against the title, tags, and content of your memory."
    )]
    pub keywords: String,
    #[schemars(
        description = "Start time for filtering memories by creation time (inclusive range, ISO 8601 format)"
    )]
    pub start_time: Option<DateTime<Utc>>,
    #[schemars(
        description = "End time for filtering memories by creation time (inclusive range, ISO 8601 format)"
    )]
    pub end_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadRulebookRequest {
    #[schemars(
        description = "The URI of the rulebook to read. This should be a valid URI pointing to a rulebook document."
    )]
    pub uri: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LocalCodeSearchRequest {
    #[schemars(
        description = "Space-separated keywords to search for in code blocks (e.g., 'kubernetes service deployment'). Searches against block names, types, content, and file paths. Blocks matching multiple keywords will be ranked higher than those matching only one keyword."
    )]
    pub keywords: String,
    #[schemars(description = "Maximum number of results to return (default: 10)")]
    pub limit: Option<u32>,
    #[schemars(
        description = "Whether to show dependencies and dependents for each matching block (default: false)"
    )]
    pub show_dependencies: Option<bool>,
}

#[tool_router(router = tool_router_remote, vis = "pub")]
impl ToolContainer {
    //     #[tool(
    //         description = "Advanced Generate/Edit devops configurations and infrastructure as code with suggested file names using a given prompt. If save_files is true, the generated files will be saved to the filesystem. The printed shell output will redact any secrets, will be replaced with a placeholder [REDACTED_SECRET:rule-id:short-hash]
    // IMPORTANT: When breaking down large projects into multiple generation steps, always include previously generated files in the 'context' parameter to maintain coherent references and consistent structure across all generated files."
    //     )]
    //     pub async fn generate_code(
    //         &self,
    //         Parameters(GenerateCodeRequest {
    //             prompt,
    //             save_files,
    //             context,
    //         }): Parameters<GenerateCodeRequest>,
    //     ) -> Result<CallToolResult, McpError> {
    //         let output_format = if save_files.unwrap_or(false) {
    //             "json"
    //         } else {
    //             "markdown"
    //         };

    //         // Convert context paths to Vec<Document>
    //         let context_documents = if let Some(context_paths) = context {
    //             context_paths
    //                 .into_iter()
    //                 .map(|path| {
    //                     let uri = format!("file://{}", path);
    //                     match std::fs::read_to_string(&path) {
    //                         Ok(content) => {
    //                             // Redact secrets in the file content
    //                             let redacted_content = self
    //                                 .get_secret_manager()
    //                                 .redact_and_store_secrets(&content, Some(&path));
    //                             SimpleDocument {
    //                                 uri,
    //                                 content: redacted_content,
    //                             }
    //                         }
    //                         Err(e) => {
    //                             warn!("Failed to read context file {}: {}", path, e);
    //                             // Add empty document with error message
    //                             SimpleDocument {
    //                                 uri,
    //                                 content: format!("Error reading file: {}", e),
    //                             }
    //                         }
    //                     }
    //                 })
    //                 .collect::<Vec<_>>()
    //         } else {
    //             Vec::new()
    //         };

    //         let client = match self.get_client() {
    //             Some(client) => client,
    //             None => {
    //                 return Ok(CallToolResult::error(vec![
    //                     Content::text("CLIENT_NOT_FOUND"),
    //                     Content::text("Client not found"),
    //                 ]));
    //             }
    //         };

    //         let response = match client
    //             .call_mcp_tool(&ToolsCallParams {
    //                 name: "generate_code".to_string(),
    //                 arguments: json!({
    //                     "prompt": prompt,
    //                     "context": context_documents,
    //                     "output_format": output_format,
    //                 }),
    //             })
    //             .await
    //         {
    //             Ok(response) => response,
    //             Err(e) => {
    //                 return Ok(CallToolResult::error(vec![
    //                     Content::text("GENERATE_CODE_ERROR"),
    //                     Content::text(format!("Failed to generate code: {}", e)),
    //                 ]));
    //             }
    //         };

    //         if save_files.unwrap_or(false) {
    //             let mut result_report = String::new();

    //             let response_text = response
    //                 .iter()
    //                 .map(|r| {
    //                     if let Some(RawTextContent { text }) = r.as_text() {
    //                         text.clone()
    //                     } else {
    //                         "".to_string()
    //                     }
    //                 })
    //                 .collect::<Vec<_>>()
    //                 .join("");

    //             let generation_result: GenerationResult = match serde_json::from_str(&response_text) {
    //                 Ok(result) => result,
    //                 Err(e) => {
    //                     error!("Failed to parse generation result: {}", e);
    //                     return Ok(CallToolResult::error(vec![
    //                         Content::text("GENERATION_PARSE_ERROR"),
    //                         Content::text(format!("Failed to parse generation result: {}", e)),
    //                     ]));
    //                 }
    //             };

    //             let mut new_files: Vec<String> = Vec::new();
    //             let mut failed_edits = Vec::new();

    //             for edit in generation_result.edits.unwrap_or_default() {
    //                 let file_path = Path::new(
    //                     edit.document_uri
    //                         .strip_prefix("file:///")
    //                         .or_else(|| edit.document_uri.strip_prefix("file://"))
    //                         .unwrap_or(&edit.document_uri),
    //                 );

    //                 // Create parent directories if they don't exist
    //                 if let Some(parent) = file_path.parent() {
    //                     if !parent.exists() {
    //                         if let Err(e) = fs::create_dir_all(parent) {
    //                             error!("Failed to create directory {}: {}", parent.display(), e);
    //                             failed_edits.push(format!(
    //                                 "Failed to create directory {} for file {}: {}\nEdit content:\n{}",
    //                                 parent.display(),
    //                                 file_path.display(),
    //                                 e,
    //                                 edit
    //                             ));
    //                             continue;
    //                         }
    //                     }
    //                 }

    //                 // Check if file exists, if not create it
    //                 if !file_path.exists() {
    //                     match fs::File::create(file_path) {
    //                         Ok(_) => {
    //                             new_files.push(file_path.to_str().unwrap_or_default().to_string());
    //                         }
    //                         Err(e) => {
    //                             error!("Failed to create file {}: {}", file_path.display(), e);
    //                             failed_edits.push(format!(
    //                                 "Failed to create file {}: {}\nEdit content:\n{}",
    //                                 file_path.display(),
    //                                 e,
    //                                 edit
    //                             ));
    //                             continue;
    //                         }
    //                     }
    //                 }

    //                 let redacted_edit = self
    //                     .get_secret_manager()
    //                     .redact_and_store_secrets(&edit.to_string(), file_path.to_str());

    //                 if edit.old_str.is_empty() {
    //                     // This is an addition to a file (appending content)
    //                     match fs::OpenOptions::new().append(true).open(file_path) {
    //                         Ok(mut file) => {
    //                             if let Err(e) = file.write_all(edit.new_str.as_bytes()) {
    //                                 error!("Failed to append to file {}: {}", file_path.display(), e);
    //                                 failed_edits.push(format!(
    //                                     "Failed to append content to file {}: {}\nEdit content:\n{}",
    //                                     file_path.display(),
    //                                     e,
    //                                     redacted_edit
    //                                 ));
    //                                 continue;
    //                             }
    //                             result_report.push_str(&format!("{}\n\n", redacted_edit));
    //                         }
    //                         Err(e) => {
    //                             error!(
    //                                 "Failed to open file for appending {}: {}",
    //                                 file_path.display(),
    //                                 e
    //                             );
    //                             failed_edits.push(format!(
    //                                 "Failed to open file {} for appending: {}\nEdit content:\n{}",
    //                                 file_path.display(),
    //                                 e,
    //                                 redacted_edit
    //                             ));
    //                             continue;
    //                         }
    //                     }
    //                 } else {
    //                     // This is a modification to a file (replacing content)
    //                     // Read the current file content
    //                     let current_content = match fs::read_to_string(file_path) {
    //                         Ok(content) => content,
    //                         Err(e) => {
    //                             error!("Failed to read file {}: {}", file_path.display(), e);
    //                             failed_edits.push(format!(
    //                                 "Failed to read file {} for content replacement: {}\nEdit content:\n{}",
    //                                 file_path.display(),
    //                                 e,
    //                                 edit
    //                             ));
    //                             continue;
    //                         }
    //                     };

    //                     // Verify that the file contains the old string
    //                     if !current_content.contains(&edit.old_str) {
    //                         error!(
    //                             "Search string not found in file {}, skipping edit: \n{}",
    //                             file_path.display(),
    //                             edit
    //                         );
    //                         failed_edits.push(format!(
    //                             "Search string not found in file {} - the file content may have changed or the search string is incorrect.\nEdit content:\n{}",
    //                             file_path.display(),
    //                             edit
    //                         ));
    //                         continue;
    //                     }

    //                     // Replace old content with new content
    //                     let updated_content = current_content.replace(&edit.old_str, &edit.new_str);
    //                     match fs::write(file_path, updated_content) {
    //                         Ok(_) => {
    //                             result_report.push_str(&format!("{}\n\n", redacted_edit));
    //                         }
    //                         Err(e) => {
    //                             error!("Failed to write to file {}: {}", file_path.display(), e);
    //                             failed_edits.push(format!(
    //                                 "Failed to write updated content to file {}: {}\nEdit content:\n{}",
    //                                 file_path.display(),
    //                                 e,
    //                                 redacted_edit
    //                             ));
    //                             continue;
    //                         }
    //                     }
    //                 }
    //             }

    //             // Build the final result report
    //             let mut final_report = String::new();

    //             if !new_files.is_empty() {
    //                 final_report.push_str(&format!("Created files: {}\n\n", new_files.join(", ")));
    //             }

    //             if !result_report.is_empty() {
    //                 final_report.push_str("Successfully applied edits:\n");
    //                 final_report.push_str(&result_report);
    //             }

    //             if !failed_edits.is_empty() {
    //                 final_report.push_str("\n❌ Failed Edits:\n");
    //                 for (i, failed_edit) in failed_edits.iter().enumerate() {
    //                     final_report.push_str(&format!("{}. {}\n", i + 1, failed_edit));
    //                 }
    //                 final_report.push_str("\nPlease review the failed edits above and take appropriate action to resolve the issues.\n");
    //             }

    //             Ok(CallToolResult::success(vec![Content::text(final_report)]))
    //         } else {
    //             Ok(CallToolResult::success(response))
    //         }
    //     }

    #[tool(
        description = "Web search for technical documentation. This includes documentation for tools, cloud providers, development frameworks, release notes, and other technical resources. searches against the url, title, description, and content of documentation chunks.
KEYWORD FORMAT REQUIREMENTS:
- Keywords should be provided as space-separated strings
- Use hyphens for compound terms (e.g., 'cloud-native', 'service-mesh')
- keywords must include version numbers if specified by the user , if not specified, use the  keyword *latest*

CORRECT EXAMPLES:
✅ keywords: 'kubernetes ingress nginx ssl'
✅ keywords: 'docker multi-stage build'

QUERY STRATEGY GUIDANCE:
- For more fine-grained queries: Use many keywords in a single call to get highly targeted results (e.g., 'kubernetes ingress nginx ssl tls' for a specific SSL setup question)
- For broader knowledge gathering: Break down your query into multiple parallel calls with fewer keywords each to cover more ground (e.g., separate calls for 'kubernetes networking', 'kubernetes storage', 'kubernetes security' instead of cramming all topics into one call)

If your goal requires understanding multiple distinct topics or technologies, make separate search calls rather than combining all keywords into one overly-specific search that may miss relevant documentation."
    )]
    pub async fn search_docs(
        &self,
        Parameters(SearchDocsRequest {
            keywords,
            exclude_keywords,
            limit,
        }): Parameters<SearchDocsRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Cap the limit to a maximum of 5
        let limit = limit.map(|l| l.min(5)).or(Some(5));

        // Split keywords into array
        let keywords_array: Vec<String> =
            keywords.split_whitespace().map(|s| s.to_string()).collect();
        let exclude_keywords_array: Option<Vec<String>> =
            exclude_keywords.map(|s| s.split_whitespace().map(|word| word.to_string()).collect());

        let client = match self.get_client() {
            Some(client) => client,
            None => {
                return Ok(CallToolResult::error(vec![
                    Content::text("CLIENT_NOT_FOUND"),
                    Content::text("Client not found"),
                ]));
            }
        };

        let response = match client
            .call_mcp_tool(&ToolsCallParams {
                name: "search_docs".to_string(),
                arguments: json!({
                    "keywords": keywords_array,
                    "exclude_keywords": exclude_keywords_array,
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

    #[tool(
        description = "Search your memory for relevant information from previous conversations and code generation steps to accelerate request fulfillment."
    )]
    pub async fn search_memory(
        &self,
        Parameters(SearchMemoryRequest {
            keywords,
            start_time,
            end_time,
        }): Parameters<SearchMemoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        // // Cap the limit to a maximum of 5
        // let limit = limit.map(|l| l.min(5)).or(Some(5));

        // Split keywords into array
        let keywords_array: Vec<String> =
            keywords.split_whitespace().map(|s| s.to_string()).collect();

        let client = match self.get_client() {
            Some(client) => client,
            None => {
                return Ok(CallToolResult::error(vec![
                    Content::text("CLIENT_NOT_FOUND"),
                    Content::text("Client not found"),
                ]));
            }
        };

        let response = match client
            .call_mcp_tool(&ToolsCallParams {
                name: "search_memory".to_string(),
                arguments: json!({
                    "keywords": keywords_array,
                    "start_time": start_time,
                    "end_time": end_time,
                    // "limit": limit,
                }),
            })
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("SEARCH_MEMORY_ERROR"),
                    Content::text(format!("Failed to search for memory: {}", e)),
                ]));
            }
        };

        Ok(CallToolResult::success(response))
    }

    #[tool(
        description = "Read and retrieve the contents of a rulebook using its URI. This tool allows you to access and read rulebooks that contain play books, guidelines, policies, or rules defined by the user."
    )]
    pub async fn read_rulebook(
        &self,
        Parameters(ReadRulebookRequest { uri }): Parameters<ReadRulebookRequest>,
    ) -> Result<CallToolResult, McpError> {
        let client = match self.get_client() {
            Some(client) => client,
            None => {
                return Ok(CallToolResult::error(vec![
                    Content::text("CLIENT_NOT_FOUND"),
                    Content::text("Client not found"),
                ]));
            }
        };

        let response = match client
            .call_mcp_tool(&ToolsCallParams {
                name: "read_rulebook".to_string(),
                arguments: json!({
                    "uri": uri,
                }),
            })
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("READ_RULEBOOK_ERROR"),
                    Content::text(format!("Failed to read rulebook: {}", e)),
                ]));
            }
        };

        Ok(CallToolResult::success(response))
    }

    #[tool(description = "Search for local code blocks using multiple keywords.
IMPORTANT: this tool ONLY search through local Terraform, Kubernetes, Dockerfile, and Github Actions code.
This tool searches through the locally indexed code blocks using text matching against names, types, content, and file paths. Blocks matching multiple keywords are ranked higher in the results. It can also show dependencies and dependents of matching blocks. If no index is found, it will build one first.")]
    pub async fn local_code_search(
        &self,
        Parameters(LocalCodeSearchRequest {
            keywords,
            limit,
            show_dependencies,
        }): Parameters<LocalCodeSearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        // First check indexing status
        if let Ok(status_str) = LocalStore::read_session_data("indexing_status.json")
            && !status_str.is_empty()
            && let Ok(status) = serde_json::from_str::<IndexingStatus>(&status_str)
            && !status.indexed
        {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "❌ Local code search is not available: {}\n\nTo enable local code search for large projects, restart the CLI with the --index-big-project flag:\n\nstakpak --index-big-project",
                status.reason
            ))]));
        }

        let index_str = match LocalStore::read_session_data("code_index.json") {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to read code index: {}", e);
                return Ok(CallToolResult::error(vec![
                    Content::text("CODE_INDEX_READ_ERROR"),
                    Content::text(format!(
                        "Failed to read code index - the project may not be indexed yet or indexing may have been skipped: {}",
                        e
                    )),
                ]));
            }
        };

        if index_str.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "❌ Local code search is not available: No code index found.\n\nThis usually means:\n1. The project is too large and indexing was skipped (use --index-big-project flag)\n2. No supported files were found in this directory\n3. The project hasn't been indexed yet\n\nRestart the CLI with --index-big-project if you want to index a large project.",
            )]));
        }

        let index_store: CodeIndex = match serde_json::from_str(&index_str) {
            Ok(index) => index,
            Err(e) => {
                return Ok(CallToolResult::error(vec![
                    Content::text("CODE_INDEX_PARSE_ERROR"),
                    Content::text(format!("Failed to parse code index: {}", e)),
                ]));
            }
        };

        let search_limit = limit.unwrap_or(10) as usize;
        let show_deps = show_dependencies.unwrap_or(false);
        let keywords_lower: Vec<String> = keywords
            .split_whitespace()
            .map(|s| s.to_lowercase())
            .collect();

        // Search through blocks
        let mut matching_blocks = Vec::new();

        for block in &index_store.index.blocks {
            let mut score = 0u32;
            let mut matched_keywords = std::collections::HashSet::new();

            // For each keyword, check matches and accumulate scores
            for keyword in &keywords_lower {
                let mut keyword_matched = false;

                // Check name match
                if let Some(name) = &block.name
                    && name.to_lowercase().contains(keyword)
                {
                    score += 10;
                    keyword_matched = true;
                }

                // Check type match
                if let Some(type_name) = &block.r#type
                    && type_name.to_lowercase().contains(keyword)
                {
                    score += 8;
                    keyword_matched = true;
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
                .get_secret_manager()
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
