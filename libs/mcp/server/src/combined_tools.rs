use rmcp::{
    Error as McpError, RoleServer, ServerHandler, model::*, schemars, service::RequestContext, tool,
};
use stakpak_api::ClientConfig;

use crate::local_tools::LocalTools;
use crate::remote_tools::{Provisioner, RemoteTools};
use crate::tool_descriptions::*;

/// Combined tools that include both local and remote functionality
#[derive(Clone)]
pub struct CombinedTools {
    local_tools: LocalTools,
    remote_tools: RemoteTools,
}

#[tool(tool_box)]
impl CombinedTools {
    pub fn new(api_config: ClientConfig, redact_secrets: bool) -> Self {
        Self {
            local_tools: LocalTools::new(redact_secrets),
            remote_tools: RemoteTools::new(api_config, redact_secrets),
        }
    }

    // Local tools delegation
    #[tool(description = RUN_COMMAND_DESCRIPTION)]
    pub async fn run_command(
        &self,
        peer: rmcp::Peer<RoleServer>,
        #[tool(param)]
        #[schemars(description = COMMAND_PARAM_DESCRIPTION)]
        command: String,
        #[tool(param)]
        #[schemars(description = WORK_DIR_PARAM_DESCRIPTION)]
        work_dir: Option<String>,
    ) -> Result<CallToolResult, McpError> {
        self.local_tools.run_command(peer, command, work_dir).await
    }

    #[tool(description = VIEW_DESCRIPTION)]
    pub fn view(
        &self,
        #[tool(param)]
        #[schemars(description = PATH_PARAM_DESCRIPTION)]
        path: String,
        #[tool(param)]
        #[schemars(description = VIEW_RANGE_PARAM_DESCRIPTION)]
        view_range: Option<[i32; 2]>,
    ) -> Result<CallToolResult, McpError> {
        self.local_tools.view(path, view_range)
    }

    #[tool(description = STR_REPLACE_DESCRIPTION)]
    pub fn str_replace(
        &self,
        #[tool(param)]
        #[schemars(description = FILE_PATH_PARAM_DESCRIPTION)]
        path: String,
        #[tool(param)]
        #[schemars(description = OLD_STR_PARAM_DESCRIPTION)]
        old_str: String,
        #[tool(param)]
        #[schemars(description = NEW_STR_PARAM_DESCRIPTION)]
        new_str: String,
    ) -> Result<CallToolResult, McpError> {
        self.local_tools.str_replace(path, old_str, new_str)
    }

    #[tool(description = CREATE_DESCRIPTION)]
    pub fn create(
        &self,
        #[tool(param)]
        #[schemars(description = CREATE_PATH_PARAM_DESCRIPTION)]
        path: String,
        #[tool(param)]
        #[schemars(description = FILE_TEXT_PARAM_DESCRIPTION)]
        file_text: String,
    ) -> Result<CallToolResult, McpError> {
        self.local_tools.create(path, file_text)
    }

    #[tool(description = INSERT_DESCRIPTION)]
    pub fn insert(
        &self,
        #[tool(param)]
        #[schemars(description = FILE_PATH_PARAM_DESCRIPTION)]
        path: String,
        #[tool(param)]
        #[schemars(description = INSERT_LINE_PARAM_DESCRIPTION)]
        insert_line: u32,
        #[tool(param)]
        #[schemars(description = INSERT_TEXT_PARAM_DESCRIPTION)]
        new_str: String,
    ) -> Result<CallToolResult, McpError> {
        self.local_tools.insert(path, insert_line, new_str)
    }

    // Remote tools delegation
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
        self.remote_tools
            .generate_code(prompt, provisioner, save_files, context)
            .await
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
        self.remote_tools.remote_code_search(query, limit).await
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
        self.remote_tools
            .local_code_search(keywords, limit, show_dependencies)
            .await
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
        self.remote_tools
            .search_docs(keywords, exclude_keywords, limit)
            .await
    }

    #[tool(description = SEARCH_MEMORY_DESCRIPTION)]
    pub async fn search_memory(
        &self,
        #[tool(param)]
        #[schemars(description = SEARCH_MEMORY_KEYWORDS_PARAM_DESCRIPTION)]
        keywords: Vec<String>,
    ) -> Result<CallToolResult, McpError> {
        self.remote_tools.search_memory(keywords).await
    }

    #[tool(description = READ_RULEBOOK_DESCRIPTION)]
    pub async fn read_rulebook(
        &self,
        #[tool(param)]
        #[schemars(description = READ_RULEBOOK_URI_PARAM_DESCRIPTION)]
        uri: String,
    ) -> Result<CallToolResult, McpError> {
        self.remote_tools.read_rulebook(uri).await
    }
}

#[tool(tool_box)]
impl ServerHandler for CombinedTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "This server provides both local tools (file operations, command execution) and remote tools (code generation, smart search) that can run commands on the system and interact with Stakpak API.".to_string(),
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
