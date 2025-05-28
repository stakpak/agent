use rmcp::{
    Error as McpError, RoleServer, ServerHandler, model::*, schemars, service::RequestContext, tool,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use stakpak_api::ToolsCallParams;
use stakpak_api::{Client, ClientConfig};
use std::fs;
use std::path::Path;
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

    #[tool(
        description = "A tool used to generate specialized devops configurations and infrastructure as code using a given prompt and provisioner (type of code to generate). This tool does not create or modify files, it only generates the code with proposed file names and content."
    )]
    async fn generate_code(
        &self,
        #[tool(param)]
        #[schemars(
            description = "The prompt to use to generate the code, this should be a detailed description of the code you want to generate"
        )]
        prompt: String,
        #[tool(param)]
        #[schemars(
            description = "The provisioner to use to generate the code one of Kubernetes, Terraform, Dockerfile, GithubActions"
        )]
        provisioner: Provisioner,
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
                name: "generate_code".to_string(),
                arguments: json!({
                    "prompt": prompt,
                    "provisioner": provisioner.to_string(),
                    "context": Vec::<serde_json::Value>::new(),
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

        Ok(CallToolResult::success(response))
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
    #[tool(
        description = "View the contents of a file or list the contents of a directory. Can read entire files or specific line ranges."
    )]
    fn view(
        &self,
        #[tool(param)]
        #[schemars(description = "The path to the file or directory to view")]
        path: String,
        #[tool(param)]
        #[schemars(
            description = "Optional line range to view [start_line, end_line]. Line numbers are 1-indexed. Use -1 for end_line to read to end of file."
        )]
        view_range: Option<[i32; 2]>,
    ) -> Result<CallToolResult, McpError> {
        let path_obj = Path::new(&path);

        if !path_obj.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_NOT_FOUND"),
                Content::text(format!("File or directory not found: {}", path)),
            ]));
        }

        if path_obj.is_dir() {
            // List directory contents
            match fs::read_dir(&path) {
                Ok(entries) => {
                    let mut result = format!("Directory listing for {}:\n", path);
                    let mut items: Vec<_> = entries.collect();
                    items.sort_by(|a, b| match (a, b) {
                        (Ok(a_entry), Ok(b_entry)) => {
                            match (
                                a_entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false),
                                b_entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false),
                            ) {
                                (true, false) => std::cmp::Ordering::Less,
                                (false, true) => std::cmp::Ordering::Greater,
                                _ => a_entry.file_name().cmp(&b_entry.file_name()),
                            }
                        }
                        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                        (Err(_), Err(_)) => std::cmp::Ordering::Equal,
                    });

                    for entry in items {
                        match entry {
                            Ok(entry) => {
                                let file_type = match entry.file_type() {
                                    Ok(ft) if ft.is_dir() => "📁",
                                    Ok(_) => "📄",
                                    Err(_) => "❓",
                                };
                                result.push_str(&format!(
                                    "{} {}\n",
                                    file_type,
                                    entry.file_name().to_string_lossy()
                                ));
                            }
                            Err(e) => {
                                result.push_str(&format!("Error reading entry: {}\n", e));
                            }
                        }
                    }
                    Ok(CallToolResult::success(vec![Content::text(result)]))
                }
                Err(e) => Ok(CallToolResult::error(vec![
                    Content::text("READ_ERROR"),
                    Content::text(format!("Cannot read directory: {}", e)),
                ])),
            }
        } else {
            // Read file contents
            match fs::read_to_string(&path) {
                Ok(content) => {
                    let result = if let Some([start, end]) = view_range {
                        let lines: Vec<&str> = content.lines().collect();
                        let start_idx = if start <= 0 { 0 } else { (start - 1) as usize };
                        let end_idx = if end == -1 {
                            lines.len()
                        } else {
                            std::cmp::min(end as usize, lines.len())
                        };

                        if start_idx >= lines.len() {
                            return Ok(CallToolResult::error(vec![
                                Content::text("INVALID_RANGE"),
                                Content::text(format!(
                                    "Start line {} is beyond file length {}",
                                    start,
                                    lines.len()
                                )),
                            ]));
                        }

                        let selected_lines = &lines[start_idx..end_idx];
                        let mut result =
                            format!("File: {} (lines {}-{})\n", path, start_idx + 1, end_idx);
                        for (i, line) in selected_lines.iter().enumerate() {
                            result.push_str(&format!("{:4}: {}\n", start_idx + i + 1, line));
                        }
                        result
                    } else {
                        let lines: Vec<&str> = content.lines().collect();
                        let mut result = format!("File: {} ({} lines)\n", path, lines.len());
                        for (i, line) in lines.iter().enumerate() {
                            result.push_str(&format!("{:4}: {}\n", i + 1, line));
                        }
                        result
                    };

                    Ok(CallToolResult::success(vec![Content::text(clip_output(
                        &result,
                    ))]))
                }
                Err(e) => Ok(CallToolResult::error(vec![
                    Content::text("READ_ERROR"),
                    Content::text(format!("Cannot read file: {}", e)),
                ])),
            }
        }
    }

    #[tool(
        description = "Replace a specific string in a file with new text. The old_str must match exactly including whitespace and indentation."
    )]
    fn str_replace(
        &self,
        #[tool(param)]
        #[schemars(description = "The path to the file to modify")]
        path: String,
        #[tool(param)]
        #[schemars(
            description = "The exact text to replace (must match exactly, including whitespace and indentation)"
        )]
        old_str: String,
        #[tool(param)]
        #[schemars(description = "The new text to insert in place of the old text")]
        new_str: String,
    ) -> Result<CallToolResult, McpError> {
        let path_obj = Path::new(&path);

        if !path_obj.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_NOT_FOUND"),
                Content::text(format!("File not found: {}", path)),
            ]));
        }

        if path_obj.is_dir() {
            return Ok(CallToolResult::error(vec![
                Content::text("IS_DIRECTORY"),
                Content::text(format!("Cannot edit directory: {}", path)),
            ]));
        }

        match fs::read_to_string(&path) {
            Ok(content) => {
                let matches: Vec<_> = content.match_indices(&old_str).collect();

                match matches.len() {
                    0 => Ok(CallToolResult::error(vec![
                        Content::text("NO_MATCH"),
                        Content::text(
                            "No match found for replacement text. Please check your text and try again.",
                        ),
                    ])),
                    1 => {
                        let new_content = content.replace(&old_str, &new_str);
                        match fs::write(&path, new_content) {
                            Ok(_) => Ok(CallToolResult::success(vec![Content::text(format!(
                                "Successfully replaced text in {}",
                                path
                            ))])),
                            Err(e) => Ok(CallToolResult::error(vec![
                                Content::text("WRITE_ERROR"),
                                Content::text(format!("Cannot write to file: {}", e)),
                            ])),
                        }
                    }
                    n => Ok(CallToolResult::error(vec![
                        Content::text("MULTIPLE_MATCHES"),
                        Content::text(format!(
                            "Found {} matches for replacement text. Please provide more context to make a unique match.",
                            n
                        )),
                    ])),
                }
            }
            Err(e) => Ok(CallToolResult::error(vec![
                Content::text("READ_ERROR"),
                Content::text(format!("Cannot read file: {}", e)),
            ])),
        }
    }

    #[tool(
        description = "Create a new file with the specified content. Will fail if file already exists."
    )]
    fn create(
        &self,
        #[tool(param)]
        #[schemars(description = "The path where the new file should be created")]
        path: String,
        #[tool(param)]
        #[schemars(description = "The content to write to the new file")]
        file_text: String,
    ) -> Result<CallToolResult, McpError> {
        let path_obj = Path::new(&path);

        if path_obj.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_EXISTS"),
                Content::text(format!("File already exists: {}", path)),
            ]));
        }

        // Create parent directories if they don't exist
        if let Some(parent) = path_obj.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Ok(CallToolResult::error(vec![
                        Content::text("CREATE_DIR_ERROR"),
                        Content::text(format!("Cannot create parent directories: {}", e)),
                    ]));
                }
            }
        }

        match fs::write(&path, file_text) {
            Ok(_) => {
                let lines = fs::read_to_string(&path)
                    .map(|content| content.lines().count())
                    .unwrap_or(0);
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Successfully created file {} with {} lines",
                    path, lines
                ))]))
            }
            Err(e) => Ok(CallToolResult::error(vec![
                Content::text("WRITE_ERROR"),
                Content::text(format!("Cannot create file: {}", e)),
            ])),
        }
    }

    #[tool(
        description = "Insert text at a specific line number in a file. Line numbers are 1-indexed."
    )]
    fn insert(
        &self,
        #[tool(param)]
        #[schemars(description = "The path to the file to modify")]
        path: String,
        #[tool(param)]
        #[schemars(description = "The line number where text should be inserted (1-indexed)")]
        insert_line: u32,
        #[tool(param)]
        #[schemars(description = "The text to insert")]
        new_str: String,
    ) -> Result<CallToolResult, McpError> {
        let path_obj = Path::new(&path);

        if !path_obj.exists() {
            return Ok(CallToolResult::error(vec![
                Content::text("FILE_NOT_FOUND"),
                Content::text(format!("File not found: {}", path)),
            ]));
        }

        if path_obj.is_dir() {
            return Ok(CallToolResult::error(vec![
                Content::text("IS_DIRECTORY"),
                Content::text(format!("Cannot edit directory: {}", path)),
            ]));
        }

        match fs::read_to_string(&path) {
            Ok(content) => {
                let mut lines: Vec<&str> = content.lines().collect();
                let insert_idx = if insert_line == 0 {
                    0
                } else {
                    (insert_line - 1) as usize
                };

                if insert_idx > lines.len() {
                    return Ok(CallToolResult::error(vec![
                        Content::text("INVALID_LINE"),
                        Content::text(format!(
                            "Line number {} is beyond file length {}",
                            insert_line,
                            lines.len()
                        )),
                    ]));
                }

                // Split new_str by lines and insert each line
                let new_lines: Vec<&str> = new_str.lines().collect();
                for (i, line) in new_lines.iter().enumerate() {
                    lines.insert(insert_idx + i, line);
                }

                let new_content = lines.join("\n");
                // Preserve original file ending (with or without final newline)
                let final_content = if content.ends_with('\n') && !new_content.ends_with('\n') {
                    format!("{}\n", new_content)
                } else {
                    new_content
                };

                match fs::write(&path, final_content) {
                    Ok(_) => Ok(CallToolResult::success(vec![Content::text(format!(
                        "Successfully inserted {} lines at line {} in {}",
                        new_lines.len(),
                        insert_line,
                        path
                    ))])),
                    Err(e) => Ok(CallToolResult::error(vec![
                        Content::text("WRITE_ERROR"),
                        Content::text(format!("Cannot write to file: {}", e)),
                    ])),
                }
            }
            Err(e) => Ok(CallToolResult::error(vec![
                Content::text("READ_ERROR"),
                Content::text(format!("Cannot read file: {}", e)),
            ])),
        }
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
