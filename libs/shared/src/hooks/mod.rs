use serde::Serialize;
use std::{collections::HashMap, fmt::Debug, fmt::Display};
use tokio::task::JoinSet;
use uuid::Uuid;

/// Hook errors
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Hook execution failed: {0}")]
    ExecutionError(String),
}

#[derive(Debug, Serialize)]
pub struct HookContext<State: Clone + Serialize> {
    pub session_id: Option<Uuid>,
    pub new_checkpoint_id: Option<Uuid>,
    pub request_id: Uuid,
    pub state: State,

    #[serde(skip)]
    background_tasks: JoinSet<Result<(), HookError>>,
}

impl<State: Clone + Serialize> Clone for HookContext<State> {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id,
            new_checkpoint_id: self.new_checkpoint_id,
            request_id: self.request_id,
            state: self.state.clone(),
            background_tasks: JoinSet::new(),
        }
    }
}

impl<State: Clone + Serialize> HookContext<State> {
    pub fn new(session_id: Option<Uuid>, state: State) -> Self {
        Self {
            session_id,
            new_checkpoint_id: None,
            request_id: Uuid::new_v4(),
            state,
            background_tasks: JoinSet::new(),
        }
    }

    pub fn set_session_id(&mut self, session_id: Uuid) {
        self.session_id = Some(session_id);
    }

    pub fn set_new_checkpoint_id(&mut self, new_checkpoint_id: Uuid) {
        self.new_checkpoint_id = Some(new_checkpoint_id);
    }
}

impl<State: Clone + Serialize> HookContext<State> {
    pub fn spawn_task<F>(&mut self, task: F)
    where
        F: Future<Output = Result<(), HookError>> + Send + 'static,
    {
        self.background_tasks.spawn(task);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LifecycleEvent {
    // Request lifecycle
    BeforeRequest,
    AfterRequest,

    // LLM interaction
    BeforeInference,
    AfterInference,

    // Tool lifecycle
    ToolCallRequested,
    BeforeToolExecution,
    AfterToolExecution,
    ToolCallAborted,

    // Errors
    Error,
}

impl Display for LifecycleEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Control flow decisions from hooks
#[derive(Debug, Default)]
pub enum HookAction {
    #[default]
    Continue,
    /// Skip remaining hooks for this event
    Skip,
    /// Abort the current operation
    Abort { reason: String },
}

impl HookAction {
    /// Convert hook action to Err on Abort
    pub fn ok(self) -> Result<(), String> {
        match self {
            HookAction::Abort { reason } => Err(reason),
            _ => Ok(()),
        }
    }
}

#[async_trait::async_trait]
pub trait Hook<State: Clone + Serialize>: Send + Sync {
    fn name(&self) -> &str;

    /// Execution priority (lower = earlier execution)
    fn priority(&self) -> u8 {
        50
    }

    async fn execute(
        &self,
        ctx: &mut HookContext<State>,
        event: &LifecycleEvent,
    ) -> Result<HookAction, HookError>;
}

#[derive(Default)]
pub struct HookRegistry<State> {
    hooks: HashMap<LifecycleEvent, Vec<Box<dyn Hook<State>>>>,
}
impl<State: Clone + Serialize> std::fmt::Debug for HookRegistry<State> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut map = f.debug_map();
        for (event, hooks) in &self.hooks {
            let hook_names: Vec<&str> = hooks.iter().map(|hook| hook.name()).collect();
            map.entry(event, &hook_names);
        }
        map.finish()
    }
}

impl<State: Clone + Serialize> HookRegistry<State> {
    pub fn register(&mut self, event: LifecycleEvent, hook: Box<dyn Hook<State>>) {
        let hooks = self.hooks.entry(event).or_default();
        hooks.push(hook);

        // Sort by priority (lower = earlier execution)
        hooks.sort_by_key(|h| h.priority());
    }

    pub async fn execute_hooks(
        &self,
        ctx: &mut HookContext<State>,
        event: &LifecycleEvent,
    ) -> Result<HookAction, HookError> {
        let Some(hooks) = self.hooks.get(event) else {
            return Ok(HookAction::Continue);
        };

        for hook in hooks {
            match hook.execute(ctx, event).await? {
                HookAction::Continue => continue,
                HookAction::Skip => return Ok(HookAction::Skip),
                HookAction::Abort { reason } => {
                    return Ok(HookAction::Abort { reason });
                }
            }
        }

        Ok(HookAction::Continue)
    }
}

/**
Usage Example

```rust
use stakpak_shared::define_hook;
use stakpak_shared::hooks::{HookAction, HookContext, HookError, LifecycleEvent, Hook};
use chrono::{DateTime, Local};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use serde::Serialize;
use std::fmt::Debug;

pub struct LoggerHook;

impl LoggerHook {
    pub fn new() -> Self {
        Self
    }
}

define_hook!(LoggerHook, "logger", async |&self, ctx: &mut HookContext<State>, event: &LifecycleEvent| {
    let timestamp: DateTime<Local> = Local::now();
    let log_message = format!(
        "[{}] LoggerHook event: {:?}, {}\n",
        timestamp.format("%Y-%m-%d %H:%M:%S%.3f"),
        event,
        serde_json::to_string(&ctx).unwrap_or_default(),
    );

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("hook_events.log")
        .await
        .map_err(|e| HookError::ExecutionError(e.to_string()))?;

    file.write_all(log_message.as_bytes())
        .await
        .map_err(|e| HookError::ExecutionError(e.to_string()))?;

    Ok(HookAction::Continue)
});
```
*/
#[macro_export]
macro_rules! define_hook {
    ($name:ident, $hook_name:expr, async |&$self:ident, $ctx:ident: &mut HookContext<$state:ty>, $event:ident: &LifecycleEvent| $body:block) => {
        #[async_trait::async_trait]
        impl Hook<$state> for $name {
            fn name(&self) -> &str {
                $hook_name
            }
            async fn execute(
                &$self,
                $ctx: &mut HookContext<$state>,
                $event: &LifecycleEvent,
            ) -> Result<HookAction, HookError> {
                $body
            }
        }
    };
}
