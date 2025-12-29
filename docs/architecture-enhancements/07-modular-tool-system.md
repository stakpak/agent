# Enhancement Proposal: Modular Tool System

## Overview

OpenCode organizes tools as separate modules with clear interfaces, making it easy to add, remove, or modify tools. Stakpak's tools are currently embedded within the MCP server implementation with less clear boundaries.

## Current Stakpak Tool Architecture

```
libs/mcp/server/src/
├── tools/
│   ├── mod.rs              # All tools defined here
│   ├── file_tools.rs       # Mixed with implementation
│   └── ...
```

Tools are tightly coupled to the MCP server and share implementation details.

## OpenCode Tool Architecture

```
packages/opencode/src/tool/
├── index.ts                # Tool registry
├── bash.ts                 # Each tool is self-contained
├── edit.ts
├── glob.ts
├── grep.ts
├── ls.ts
├── read.ts
├── write.ts
├── fetch.ts
├── mcp.ts                  # MCP tool wrapper
└── ...
```

Each tool is:
- Self-contained with its own schema
- Independently testable
- Easy to enable/disable
- Documented inline

## Proposed Enhancement

### Tool Trait Definition

```rust
// libs/shared/src/tools/mod.rs
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique identifier for the tool
    fn name(&self) -> &str;
    
    /// Human-readable description
    fn description(&self) -> &str;
    
    /// JSON Schema for the tool's parameters
    fn parameters_schema(&self) -> serde_json::Value;
    
    /// Execute the tool with given parameters
    async fn execute(&self, ctx: &ToolContext, params: serde_json::Value) -> Result<ToolResult>;
    
    /// Whether this tool requires user approval
    fn requires_approval(&self) -> bool { true }
    
    /// Tool category for grouping
    fn category(&self) -> ToolCategory { ToolCategory::General }
    
    /// Whether the tool is enabled by default
    fn enabled_by_default(&self) -> bool { true }
}

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub project_root: PathBuf,
    pub cwd: PathBuf,
    pub session_id: String,
    pub event_bus: Arc<EventBus>,
    pub secret_manager: Arc<SecretManager>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    FileSystem,
    Shell,
    Search,
    Network,
    Git,
    General,
}
```

### Tool Registry

```rust
// libs/shared/src/tools/registry.rs
use std::collections::HashMap;
use std::sync::Arc;

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    enabled: HashMap<String, bool>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
            enabled: HashMap::new(),
        };
        
        // Register built-in tools
        registry.register(Arc::new(ViewTool::new()));
        registry.register(Arc::new(CreateTool::new()));
        registry.register(Arc::new(StrReplaceTool::new()));
        registry.register(Arc::new(RemoveTool::new()));
        registry.register(Arc::new(RunCommandTool::new()));
        registry.register(Arc::new(RunCommandTaskTool::new()));
        registry.register(Arc::new(SearchDocsTool::new()));
        registry.register(Arc::new(ViewWebPageTool::new()));
        registry.register(Arc::new(GeneratePasswordTool::new()));
        
        registry
    }
    
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        let enabled = tool.enabled_by_default();
        self.tools.insert(name.clone(), tool);
        self.enabled.insert(name, enabled);
    }
    
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        if self.is_enabled(name) {
            self.tools.get(name).cloned()
        } else {
            None
        }
    }
    
    pub fn enable(&mut self, name: &str) {
        self.enabled.insert(name.to_string(), true);
    }
    
    pub fn disable(&mut self, name: &str) {
        self.enabled.insert(name.to_string(), false);
    }
    
    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.get(name).copied().unwrap_or(false)
    }
    
    pub fn list(&self) -> Vec<&dyn Tool> {
        self.tools.values()
            .filter(|t| self.is_enabled(t.name()))
            .map(|t| t.as_ref())
            .collect()
    }
    
    pub fn list_by_category(&self, category: ToolCategory) -> Vec<&dyn Tool> {
        self.list().into_iter()
            .filter(|t| t.category() == category)
            .collect()
    }
    
    /// Generate MCP tool definitions
    pub fn to_mcp_tools(&self) -> Vec<rmcp::model::Tool> {
        self.list().iter()
            .map(|tool| rmcp::model::Tool {
                name: tool.name().to_string(),
                description: Some(tool.description().to_string()),
                input_schema: tool.parameters_schema(),
            })
            .collect()
    }
}
```

### Example Tool Implementations

```rust
// libs/shared/src/tools/builtin/view.rs
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ViewParams {
    /// Path to the file or directory to view
    pub path: String,
    /// Optional line range [start, end]
    pub view_range: Option<[i32; 2]>,
    /// Display as tree structure
    pub tree: Option<bool>,
    // Remote connection params
    pub remote: Option<String>,
    pub password: Option<String>,
    pub private_key_path: Option<String>,
}

pub struct ViewTool;

impl ViewTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ViewTool {
    fn name(&self) -> &str { "view" }
    
    fn description(&self) -> &str {
        "View the contents of a local or remote file/directory"
    }
    
    fn parameters_schema(&self) -> serde_json::Value {
        schemars::schema_for!(ViewParams).into()
    }
    
    fn requires_approval(&self) -> bool { false }
    
    fn category(&self) -> ToolCategory { ToolCategory::FileSystem }
    
    async fn execute(&self, ctx: &ToolContext, params: serde_json::Value) -> Result<ToolResult> {
        let params: ViewParams = serde_json::from_value(params)?;
        
        let path = if params.remote.is_some() {
            // Handle remote path
            RemotePath::new(&params.path, params.remote.as_deref())?
        } else {
            LocalPath::resolve(&ctx.cwd, &params.path)?
        };
        
        let content = if path.is_dir() {
            if params.tree.unwrap_or(false) {
                view_tree(&path, 3).await?
            } else {
                view_directory(&path).await?
            }
        } else {
            view_file(&path, params.view_range).await?
        };
        
        // Redact secrets
        let content = ctx.secret_manager.redact(&content);
        
        Ok(ToolResult {
            success: true,
            output: content,
            metadata: None,
        })
    }
}
```

```rust
// libs/shared/src/tools/builtin/run_command.rs
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunCommandParams {
    /// The shell command to execute
    pub command: String,
    /// Optional description
    pub description: Option<String>,
    /// Timeout in seconds
    pub timeout: Option<u64>,
    /// Remote connection string
    pub remote: Option<String>,
    pub password: Option<String>,
    pub private_key_path: Option<String>,
}

pub struct RunCommandTool;

#[async_trait]
impl Tool for RunCommandTool {
    fn name(&self) -> &str { "run_command" }
    
    fn description(&self) -> &str {
        "Execute a shell command on local or remote systems"
    }
    
    fn parameters_schema(&self) -> serde_json::Value {
        schemars::schema_for!(RunCommandParams).into()
    }
    
    fn requires_approval(&self) -> bool { true }
    
    fn category(&self) -> ToolCategory { ToolCategory::Shell }
    
    async fn execute(&self, ctx: &ToolContext, params: serde_json::Value) -> Result<ToolResult> {
        let params: RunCommandParams = serde_json::from_value(params)?;
        
        // Restore secrets in command
        let command = ctx.secret_manager.restore(&params.command);
        
        let output = if let Some(remote) = params.remote {
            execute_remote(&remote, &command, params.timeout).await?
        } else {
            execute_local(&command, &ctx.cwd, params.timeout).await?
        };
        
        // Redact secrets in output
        let output = ctx.secret_manager.redact(&output);
        
        Ok(ToolResult {
            success: true,
            output,
            metadata: None,
        })
    }
}
```

```rust
// libs/shared/src/tools/builtin/str_replace.rs
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StrReplaceParams {
    /// Path to the file to modify
    pub path: String,
    /// Exact text to replace
    pub old_str: String,
    /// New text to insert
    pub new_str: String,
    /// Replace all occurrences
    pub replace_all: Option<bool>,
    // Remote params...
}

pub struct StrReplaceTool {
    backup_manager: Arc<FileBackupManager>,
}

#[async_trait]
impl Tool for StrReplaceTool {
    fn name(&self) -> &str { "str_replace" }
    
    fn description(&self) -> &str {
        "Replace a specific string in a file with new text"
    }
    
    fn parameters_schema(&self) -> serde_json::Value {
        schemars::schema_for!(StrReplaceParams).into()
    }
    
    fn requires_approval(&self) -> bool { true }
    
    fn category(&self) -> ToolCategory { ToolCategory::FileSystem }
    
    async fn execute(&self, ctx: &ToolContext, params: serde_json::Value) -> Result<ToolResult> {
        let params: StrReplaceParams = serde_json::from_value(params)?;
        
        // Restore secrets in old_str and new_str
        let old_str = ctx.secret_manager.restore(&params.old_str);
        let new_str = ctx.secret_manager.restore(&params.new_str);
        
        let path = LocalPath::resolve(&ctx.cwd, &params.path)?;
        
        // Create backup before modification
        let backup_path = self.backup_manager.backup(&path).await?;
        
        // Perform replacement
        let content = tokio::fs::read_to_string(&path).await?;
        
        if !content.contains(&old_str) {
            return Ok(ToolResult {
                success: false,
                output: "STRING_NOT_FOUND: The specified text was not found in the file".to_string(),
                metadata: Some(json!({ "backup_path": backup_path })),
            });
        }
        
        let new_content = if params.replace_all.unwrap_or(false) {
            content.replace(&old_str, &new_str)
        } else {
            content.replacen(&old_str, &new_str, 1)
        };
        
        tokio::fs::write(&path, &new_content).await?;
        
        // Publish event for undo tracking
        ctx.event_bus.publish("file.modified", json!({
            "path": path,
            "backup_path": backup_path,
        }))?;
        
        Ok(ToolResult {
            success: true,
            output: format!("Successfully replaced text in {}", path.display()),
            metadata: Some(json!({ "backup_path": backup_path })),
        })
    }
}
```

### Tool Configuration

```rust
// libs/shared/src/tools/config.rs

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    /// Explicitly enabled tools (overrides defaults)
    pub enabled: Option<Vec<String>>,
    /// Explicitly disabled tools
    pub disabled: Option<Vec<String>>,
    /// Tools that don't require approval
    pub auto_approve: Option<Vec<String>>,
    /// Tool-specific configuration
    pub tool_config: HashMap<String, serde_json::Value>,
}

impl ToolsConfig {
    pub fn apply_to_registry(&self, registry: &mut ToolRegistry) {
        // Disable all if explicit enabled list provided
        if let Some(enabled) = &self.enabled {
            for tool in registry.list() {
                registry.disable(tool.name());
            }
            for name in enabled {
                registry.enable(name);
            }
        }
        
        // Apply disabled list
        if let Some(disabled) = &self.disabled {
            for name in disabled {
                registry.disable(name);
            }
        }
    }
}
```

### MCP Server Integration

```rust
// libs/mcp/server/src/lib.rs
use stakpak_shared::tools::{ToolRegistry, ToolContext};

pub struct McpServer {
    tool_registry: Arc<ToolRegistry>,
    tool_context: ToolContext,
}

impl McpServer {
    pub fn new(tool_registry: Arc<ToolRegistry>, tool_context: ToolContext) -> Self {
        Self { tool_registry, tool_context }
    }
    
    pub fn list_tools(&self) -> Vec<rmcp::model::Tool> {
        self.tool_registry.to_mcp_tools()
    }
    
    pub async fn call_tool(&self, name: &str, params: serde_json::Value) -> Result<String> {
        let tool = self.tool_registry.get(name)
            .ok_or_else(|| anyhow!("Unknown tool: {}", name))?;
        
        let result = tool.execute(&self.tool_context, params).await?;
        
        Ok(result.output)
    }
}
```

## Directory Structure

```
libs/shared/src/tools/
├── mod.rs                    # Tool trait, ToolResult, ToolContext
├── registry.rs               # ToolRegistry
├── config.rs                 # ToolsConfig
└── builtin/
    ├── mod.rs                # Re-exports all tools
    ├── view.rs
    ├── create.rs
    ├── str_replace.rs
    ├── remove.rs
    ├── run_command.rs
    ├── run_command_task.rs
    ├── search_docs.rs
    ├── view_web_page.rs
    ├── generate_password.rs
    ├── search_memory.rs
    └── read_rulebook.rs
```

## Benefits

1. **Modularity**: Each tool is self-contained and independently testable
2. **Discoverability**: Clear interface makes it easy to understand tools
3. **Extensibility**: Add new tools without modifying existing code
4. **Configuration**: Enable/disable tools per project or globally
5. **Type Safety**: Strong typing with JSON Schema validation
6. **Reusability**: Tools can be used by MCP server, CLI, or API

## Implementation Effort

| Task | Effort | Priority |
|------|--------|----------|
| Tool Trait | 1 day | High |
| ToolRegistry | 1 day | High |
| Migrate Existing Tools | 3-4 days | High |
| Tool Configuration | 1 day | Medium |
| MCP Integration | 1 day | High |
| Documentation | 1 day | Medium |

## Files to Create/Modify

```
libs/shared/src/
├── tools/                    # NEW
│   ├── mod.rs
│   ├── registry.rs
│   ├── config.rs
│   └── builtin/
│       └── ... (one file per tool)

libs/mcp/server/src/
├── lib.rs                    # MODIFY: use ToolRegistry
└── tools/                    # REMOVE: migrate to shared

cli/src/
└── main.rs                   # MODIFY: initialize ToolRegistry
```

## Migration Strategy

1. Create Tool trait and ToolRegistry in libs/shared
2. Implement one tool (e.g., view) as proof of concept
3. Migrate remaining tools one by one
4. Update MCP server to use ToolRegistry
5. Add tool configuration support
6. Remove old tool implementations
