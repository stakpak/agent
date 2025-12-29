# Enhancement Proposal: Project Instance & Context Management

## Overview

OpenCode uses a `Project` and `Instance` pattern to manage project-level state, configuration, and context. This provides clean isolation between different projects and sessions. Stakpak currently has a more ad-hoc approach to context management.

## Current Stakpak Architecture

```rust
// cli/src/main.rs
#[tokio::main]
async fn main() {
    let config = AppConfig::load(&profile_name, cli.config_path.as_deref())?;
    let local_context = analyze_local_context(&config).await.ok();
    
    // Context is passed around as separate parameters
    let client = match config.provider {
        ProviderType::Remote => Arc::new(RemoteClient::new(&config.clone().into())?),
        ProviderType::Local => Arc::new(LocalClient::new(LocalClientConfig { ... })?),
    };
}

// tui/src/app.rs
pub struct App {
    pub config: AppConfig,
    pub local_context: Option<LocalContext>,
    // ... many fields for different concerns
}
```

## OpenCode Project/Instance Architecture

```typescript
// packages/opencode/src/project/instance.ts
export namespace Instance {
  export let project: string
  export let worktree: string
  export let directory: string
  
  const state = Instance.state(async () => {
    // Lazy-loaded, project-scoped state
  })
  
  export async function provide<T>(opts: {
    directory: string
    fn: () => Promise<T>
  }): Promise<T> {
    // Set up project context for the duration of fn()
  }
}

// packages/opencode/src/project/project.ts
export namespace Project {
  export async function init(directory: string) {
    // Initialize .opencode directory
    // Load project config
    // Set up file watchers
  }
}
```

## Proposed Enhancement

### Project Context Structure

```rust
// libs/shared/src/project/mod.rs
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ProjectContext {
    /// Root directory of the project
    pub root: PathBuf,
    /// Git worktree (if different from root)
    pub worktree: PathBuf,
    /// Current working directory
    pub cwd: PathBuf,
    /// Project-specific configuration
    pub config: ProjectConfig,
    /// Project state (sessions, history, etc.)
    state: Arc<RwLock<ProjectState>>,
}

impl ProjectContext {
    pub async fn discover(start_dir: &Path) -> Result<Self> {
        // Walk up to find .stakpak or .git directory
        let root = Self::find_project_root(start_dir)?;
        let worktree = Self::find_git_worktree(&root)?;
        let config = ProjectConfig::load(&root)?;
        let state = ProjectState::load(&root).await?;
        
        Ok(Self {
            root,
            worktree,
            cwd: start_dir.to_path_buf(),
            config,
            state: Arc::new(RwLock::new(state)),
        })
    }
    
    pub fn stakpak_dir(&self) -> PathBuf {
        self.root.join(".stakpak")
    }
    
    pub fn sessions_dir(&self) -> PathBuf {
        self.stakpak_dir().join("sessions")
    }
    
    pub fn backups_dir(&self) -> PathBuf {
        self.stakpak_dir().join("backups")
    }
    
    fn find_project_root(start: &Path) -> Result<PathBuf> {
        let mut current = start.to_path_buf();
        loop {
            if current.join(".stakpak").exists() || current.join(".git").exists() {
                return Ok(current);
            }
            if !current.pop() {
                // No project root found, use start directory
                return Ok(start.to_path_buf());
            }
        }
    }
}
```

### Project Configuration

```rust
// libs/shared/src/project/config.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    /// Project name (defaults to directory name)
    pub name: Option<String>,
    /// Default model for this project
    pub model: Option<String>,
    /// Project-specific MCP servers
    pub mcp_servers: Vec<McpServerConfig>,
    /// Files/directories to ignore
    pub ignore: Vec<String>,
    /// Custom system prompt additions
    pub system_prompt: Option<String>,
    /// Enabled tools for this project
    pub enabled_tools: Option<Vec<String>>,
    /// Disabled tools for this project
    pub disabled_tools: Option<Vec<String>>,
    /// Project-specific rulebooks
    pub rulebooks: Option<Vec<String>>,
}

impl ProjectConfig {
    pub fn load(project_root: &Path) -> Result<Self> {
        let config_path = project_root.join(".stakpak").join("config.toml");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            Ok(toml::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }
    
    pub fn save(&self, project_root: &Path) -> Result<()> {
        let config_dir = project_root.join(".stakpak");
        std::fs::create_dir_all(&config_dir)?;
        let config_path = config_dir.join("config.toml");
        let content = toml::to_string_pretty(self)?;
        std::fs::write(config_path, content)?;
        Ok(())
    }
}
```

### Project State

```rust
// libs/shared/src/project/state.rs
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectState {
    /// Active session ID
    pub active_session: Option<String>,
    /// Session metadata (not full content)
    pub sessions: HashMap<String, SessionMetadata>,
    /// File change history for undo/redo
    pub file_history: FileHistory,
    /// Checkpoint data
    pub checkpoints: Vec<Checkpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub id: String,
    pub name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileHistory {
    /// Stack of file changes for undo
    pub undo_stack: Vec<FileChange>,
    /// Stack of undone changes for redo
    pub redo_stack: Vec<FileChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: PathBuf,
    pub change_type: FileChangeType,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileChangeType {
    Created,
    Modified { original_content: String },
    Deleted { original_content: String },
}
```

### Instance Manager

```rust
// libs/shared/src/project/instance.rs
use std::sync::Arc;
use tokio::sync::RwLock;
use once_cell::sync::OnceCell;

static INSTANCE: OnceCell<Arc<RwLock<Instance>>> = OnceCell::new();

pub struct Instance {
    pub project: ProjectContext,
    pub global_config: AppConfig,
    pub auth_manager: AuthManager,
    pub event_bus: EventBus,
    pub plugin_registry: PluginRegistry,
}

impl Instance {
    pub async fn init(directory: &Path, global_config: AppConfig) -> Result<Arc<RwLock<Self>>> {
        let project = ProjectContext::discover(directory).await?;
        let auth_manager = AuthManager::new(&global_config.data_dir())?;
        let event_bus = EventBus::new();
        let plugin_registry = PluginRegistry::new();
        
        let instance = Arc::new(RwLock::new(Self {
            project,
            global_config,
            auth_manager,
            event_bus,
            plugin_registry,
        }));
        
        INSTANCE.set(instance.clone())
            .map_err(|_| anyhow!("Instance already initialized"))?;
        
        Ok(instance)
    }
    
    pub fn get() -> Result<Arc<RwLock<Self>>> {
        INSTANCE.get()
            .cloned()
            .ok_or_else(|| anyhow!("Instance not initialized"))
    }
    
    /// Run a function with a specific project context
    pub async fn with_project<F, T>(directory: &Path, f: F) -> Result<T>
    where
        F: FnOnce(&mut Instance) -> Result<T>,
    {
        let instance = Self::get()?;
        let mut guard = instance.write().await;
        
        // Temporarily switch project context
        let original_project = guard.project.clone();
        guard.project = ProjectContext::discover(directory).await?;
        
        let result = f(&mut guard);
        
        // Restore original context
        guard.project = original_project;
        
        result
    }
}
```

### Scoped State Pattern

```rust
// libs/shared/src/project/scoped_state.rs
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Project-scoped lazy state
pub struct ScopedState {
    states: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl ScopedState {
    pub fn new() -> Self {
        Self {
            states: RwLock::new(HashMap::new()),
        }
    }
    
    /// Get or initialize state of type T
    pub async fn get_or_init<T, F, Fut>(&self, init: F) -> Arc<T>
    where
        T: Send + Sync + 'static,
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let type_id = TypeId::of::<T>();
        
        // Check if already initialized
        {
            let states = self.states.read().await;
            if let Some(state) = states.get(&type_id) {
                return state.clone().downcast::<T>().unwrap();
            }
        }
        
        // Initialize
        let value = Arc::new(init().await);
        
        // Store
        {
            let mut states = self.states.write().await;
            states.insert(type_id, value.clone());
        }
        
        value
    }
}

// Usage example
impl ProjectContext {
    pub async fn code_index(&self) -> Arc<CodeIndex> {
        self.scoped_state.get_or_init(|| async {
            CodeIndex::build(&self.root).await
        }).await
    }
    
    pub async fn file_watcher(&self) -> Arc<FileWatcher> {
        self.scoped_state.get_or_init(|| async {
            FileWatcher::new(&self.root).await
        }).await
    }
}
```

### Integration with CLI

```rust
// cli/src/main.rs
#[tokio::main]
async fn main() {
    let config = AppConfig::load(&profile_name, cli.config_path.as_deref())?;
    
    // Initialize instance with project context
    let cwd = std::env::current_dir()?;
    let instance = Instance::init(&cwd, config).await?;
    
    match cli.command {
        Some(command) => {
            command.run(instance).await?;
        }
        None => {
            // Start TUI with instance
            tui::run(instance).await?;
        }
    }
}
```

### Integration with TUI

```rust
// tui/src/app.rs
pub struct App {
    instance: Arc<RwLock<Instance>>,
    // UI-specific state only
    ui_state: UiState,
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl App {
    pub async fn new(instance: Arc<RwLock<Instance>>) -> Result<Self> {
        Ok(Self {
            instance,
            ui_state: UiState::default(),
            terminal: setup_terminal()?,
        })
    }
    
    pub async fn project(&self) -> ProjectContext {
        self.instance.read().await.project.clone()
    }
    
    pub async fn config(&self) -> AppConfig {
        self.instance.read().await.global_config.clone()
    }
}
```

## Benefits

1. **Clean Separation**: Project state vs global state vs UI state
2. **Lazy Loading**: State initialized only when needed
3. **Context Isolation**: Different projects don't interfere
4. **Testability**: Easy to mock Instance for tests
5. **Consistency**: Single source of truth for project context

## Directory Structure

```
project/
├── .stakpak/
│   ├── config.toml          # Project-specific config
│   ├── state.json           # Project state
│   ├── sessions/            # Session data
│   │   ├── abc123.json
│   │   └── def456.json
│   ├── backups/             # File backups for undo
│   │   └── {uuid}/
│   └── cache/               # Code index, etc.
│       └── code_index.json
└── ... (project files)
```

## Implementation Effort

| Task | Effort | Priority |
|------|--------|----------|
| ProjectContext | 1-2 days | High |
| ProjectConfig | 1 day | High |
| ProjectState | 1-2 days | High |
| Instance Manager | 1-2 days | High |
| ScopedState | 1 day | Medium |
| CLI Integration | 1 day | High |
| TUI Refactor | 2-3 days | Medium |

## Files to Create/Modify

```
libs/shared/src/
├── project/                  # NEW
│   ├── mod.rs
│   ├── context.rs
│   ├── config.rs
│   ├── state.rs
│   ├── instance.rs
│   └── scoped_state.rs

cli/src/
└── main.rs                   # MODIFY: use Instance

tui/src/
└── app.rs                    # MODIFY: use Instance
```

## Migration Strategy

1. Create project module with new structures
2. Add Instance initialization to CLI
3. Gradually migrate App fields to use Instance
4. Add project-specific config support
5. Implement undo/redo with FileHistory
6. Add session management to ProjectState
