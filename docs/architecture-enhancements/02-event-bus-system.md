# Enhancement Proposal: Event Bus System

## Overview

OpenCode uses an event bus (`Bus`) for decoupled communication between components. Stakpak currently uses direct function calls and tight coupling between modules. This proposal details a comprehensive event-driven architecture for Stakpak.

## Current Stakpak Architecture

```rust
// Direct coupling example in tui/src/services/handlers/
impl MessageHandler {
    pub async fn handle(&mut self, app: &mut App) {
        // Direct calls to other services - tight coupling!
        app.session_service.add_message(...);
        app.render_service.update(...);
        app.telemetry.track(...);
    }
}
```

### Problems with Current Approach

1. **Circular Dependencies**: Services need references to each other
2. **Testing Difficulty**: Can't test handlers without full App context
3. **Modification Risk**: Changing one service affects many others
4. **No Async Decoupling**: Synchronous calls block the event loop
5. **No Event History**: Can't replay or audit what happened

## OpenCode Event Bus Architecture

```typescript
// packages/opencode/src/bus/index.ts
export namespace Bus {
  export const Event = {
    Session: {
      Created: BusEvent.define("session.created", SessionSchema),
      Updated: BusEvent.define("session.updated", SessionSchema),
    },
    Message: {
      Created: BusEvent.define("message.created", MessageSchema),
      Streaming: BusEvent.define("message.streaming", StreamSchema),
    },
    // ...
  }
}

// Usage
Bus.publish(Bus.Event.Session.Created, { id: "123", ... });
Bus.subscribe(Bus.Event.Message.Created, (msg) => { ... });
```

---

## Proposed Enhancement

### Core Event Bus Implementation

```rust
// libs/shared/src/event_bus/mod.rs
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};

/// Maximum number of events to buffer per channel
const DEFAULT_CHANNEL_CAPACITY: usize = 256;

/// Unique identifier for a subscription
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

/// Event metadata attached to every event
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventMetadata {
    pub id: uuid::Uuid,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub source: String,
    pub correlation_id: Option<uuid::Uuid>,
}

impl EventMetadata {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            source: source.into(),
            correlation_id: None,
        }
    }
    
    pub fn with_correlation(mut self, id: uuid::Uuid) -> Self {
        self.correlation_id = Some(id);
        self
    }
}

/// Wrapper for events with metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventEnvelope<T> {
    pub metadata: EventMetadata,
    pub payload: T,
}

/// The main event bus structure
pub struct EventBus {
    /// Typed channels for each event type
    channels: RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
    /// String-keyed channels for dynamic events
    dynamic_channels: RwLock<HashMap<String, broadcast::Sender<serde_json::Value>>>,
    /// Subscription counter for generating unique IDs
    subscription_counter: std::sync::atomic::AtomicU64,
    /// Event history for replay (optional, bounded)
    history: RwLock<EventHistory>,
    /// Global event interceptors
    interceptors: RwLock<Vec<Arc<dyn EventInterceptor>>>,
    /// Channel capacity
    capacity: usize,
}

impl EventBus {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CHANNEL_CAPACITY)
    }
    
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
            dynamic_channels: RwLock::new(HashMap::new()),
            subscription_counter: std::sync::atomic::AtomicU64::new(0),
            history: RwLock::new(EventHistory::new(1000)),
            interceptors: RwLock::new(Vec::new()),
            capacity,
        }
    }
    
    /// Publish a typed event
    pub async fn publish<E: Event>(&self, event: E) -> Result<(), EventBusError> {
        let metadata = EventMetadata::new(std::any::type_name::<E>());
        self.publish_with_metadata(event, metadata).await
    }
    
    /// Publish with custom metadata (for correlation tracking)
    pub async fn publish_with_metadata<E: Event>(
        &self,
        event: E,
        metadata: EventMetadata,
    ) -> Result<(), EventBusError> {
        let envelope = EventEnvelope {
            metadata: metadata.clone(),
            payload: event.clone(),
        };
        
        // Run interceptors
        for interceptor in self.interceptors.read().await.iter() {
            interceptor.on_publish(&metadata, &serde_json::to_value(&event)?);
        }
        
        // Store in history
        self.history.write().await.push(
            E::NAME,
            serde_json::to_value(&envelope)?,
        );
        
        // Get or create channel
        let type_id = TypeId::of::<E>();
        let channels = self.channels.read().await;
        
        if let Some(sender) = channels.get(&type_id) {
            let sender = sender
                .downcast_ref::<broadcast::Sender<EventEnvelope<E>>>()
                .ok_or(EventBusError::TypeMismatch)?;
            
            match sender.send(envelope) {
                Ok(count) => {
                    debug!(event = E::NAME, subscribers = count, "Event published");
                }
                Err(_) => {
                    // No subscribers - this is okay
                    debug!(event = E::NAME, "Event published with no subscribers");
                }
            }
        }
        
        Ok(())
    }
    
    /// Subscribe to a typed event
    pub async fn subscribe<E: Event>(
        &self,
    ) -> Result<EventSubscription<E>, EventBusError> {
        let type_id = TypeId::of::<E>();
        
        // Get or create channel
        let mut channels = self.channels.write().await;
        let sender = channels
            .entry(type_id)
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel::<EventEnvelope<E>>(self.capacity);
                Box::new(tx)
            })
            .downcast_ref::<broadcast::Sender<EventEnvelope<E>>>()
            .ok_or(EventBusError::TypeMismatch)?;
        
        let receiver = sender.subscribe();
        let id = SubscriptionId(
            self.subscription_counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        );
        
        Ok(EventSubscription {
            id,
            receiver,
            event_name: E::NAME,
        })
    }
    
    /// Subscribe with a callback (spawns a task)
    pub async fn subscribe_with_handler<E, F, Fut>(
        &self,
        handler: F,
    ) -> Result<SubscriptionId, EventBusError>
    where
        E: Event,
        F: Fn(EventEnvelope<E>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let mut subscription = self.subscribe::<E>().await?;
        let id = subscription.id;
        
        tokio::spawn(async move {
            while let Ok(envelope) = subscription.recv().await {
                handler(envelope).await;
            }
        });
        
        Ok(id)
    }
    
    /// Publish a dynamic (string-keyed) event
    pub async fn publish_dynamic(
        &self,
        event_name: &str,
        data: serde_json::Value,
    ) -> Result<(), EventBusError> {
        let channels = self.dynamic_channels.read().await;
        
        if let Some(sender) = channels.get(event_name) {
            let _ = sender.send(data);
        }
        
        Ok(())
    }
    
    /// Subscribe to dynamic events
    pub async fn subscribe_dynamic(
        &self,
        event_name: &str,
    ) -> broadcast::Receiver<serde_json::Value> {
        let mut channels = self.dynamic_channels.write().await;
        let sender = channels
            .entry(event_name.to_string())
            .or_insert_with(|| broadcast::channel(self.capacity).0);
        
        sender.subscribe()
    }
    
    /// Add a global interceptor
    pub async fn add_interceptor(&self, interceptor: Arc<dyn EventInterceptor>) {
        self.interceptors.write().await.push(interceptor);
    }
    
    /// Get event history for replay
    pub async fn get_history(&self, event_name: &str, limit: usize) -> Vec<serde_json::Value> {
        self.history.read().await.get(event_name, limit)
    }
    
    /// Replay events to a new subscriber
    pub async fn replay<E: Event>(
        &self,
        subscription: &mut EventSubscription<E>,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<EventEnvelope<E>>, EventBusError> {
        let history = self.history.read().await;
        let events = history.get_since(E::NAME, since);
        
        events
            .into_iter()
            .map(|v| serde_json::from_value(v).map_err(EventBusError::from))
            .collect()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Subscription handle for receiving events
pub struct EventSubscription<E: Event> {
    pub id: SubscriptionId,
    receiver: broadcast::Receiver<EventEnvelope<E>>,
    event_name: &'static str,
}

impl<E: Event> EventSubscription<E> {
    pub async fn recv(&mut self) -> Result<EventEnvelope<E>, EventBusError> {
        self.receiver
            .recv()
            .await
            .map_err(|e| EventBusError::ReceiveError(e.to_string()))
    }
    
    /// Convert to a stream for async iteration
    pub fn into_stream(self) -> impl futures::Stream<Item = EventEnvelope<E>> {
        tokio_stream::wrappers::BroadcastStream::new(self.receiver)
            .filter_map(|r| async { r.ok() })
    }
}

/// Trait for event types
pub trait Event: Clone + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static {
    const NAME: &'static str;
    const CATEGORY: EventCategory;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCategory {
    Session,
    Message,
    Tool,
    Provider,
    File,
    System,
    User,
}

/// Interceptor for cross-cutting concerns (logging, metrics, etc.)
#[async_trait::async_trait]
pub trait EventInterceptor: Send + Sync {
    fn on_publish(&self, metadata: &EventMetadata, payload: &serde_json::Value);
    fn on_subscribe(&self, event_name: &str, subscription_id: SubscriptionId);
}

/// Event history for replay functionality
struct EventHistory {
    events: HashMap<String, Vec<(chrono::DateTime<chrono::Utc>, serde_json::Value)>>,
    max_per_event: usize,
}

impl EventHistory {
    fn new(max_per_event: usize) -> Self {
        Self {
            events: HashMap::new(),
            max_per_event,
        }
    }
    
    fn push(&mut self, event_name: &str, value: serde_json::Value) {
        let events = self.events.entry(event_name.to_string()).or_default();
        events.push((chrono::Utc::now(), value));
        
        // Trim old events
        if events.len() > self.max_per_event {
            events.remove(0);
        }
    }
    
    fn get(&self, event_name: &str, limit: usize) -> Vec<serde_json::Value> {
        self.events
            .get(event_name)
            .map(|events| {
                events
                    .iter()
                    .rev()
                    .take(limit)
                    .map(|(_, v)| v.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    
    fn get_since(
        &self,
        event_name: &str,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Vec<serde_json::Value> {
        self.events
            .get(event_name)
            .map(|events| {
                events
                    .iter()
                    .filter(|(ts, _)| *ts >= since)
                    .map(|(_, v)| v.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Event bus errors
#[derive(Debug, thiserror::Error)]
pub enum EventBusError {
    #[error("Type mismatch in event channel")]
    TypeMismatch,
    #[error("Failed to receive event: {0}")]
    ReceiveError(String),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}
```

---

### Comprehensive Event Definitions

```rust
// libs/shared/src/event_bus/events.rs
use super::{Event, EventCategory};
use serde::{Deserialize, Serialize};

// ============================================================================
// SESSION EVENTS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreated {
    pub session_id: String,
    pub name: Option<String>,
    pub model: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Event for SessionCreated {
    const NAME: &'static str = "session.created";
    const CATEGORY: EventCategory = EventCategory::Session;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResumed {
    pub session_id: String,
    pub checkpoint_id: Option<String>,
    pub message_count: usize,
}

impl Event for SessionResumed {
    const NAME: &'static str = "session.resumed";
    const CATEGORY: EventCategory = EventCategory::Session;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEnded {
    pub session_id: String,
    pub reason: SessionEndReason,
    pub duration_secs: u64,
    pub message_count: usize,
    pub tool_calls: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionEndReason {
    UserExit,
    Completed,
    Error(String),
    Timeout,
}

impl Event for SessionEnded {
    const NAME: &'static str = "session.ended";
    const CATEGORY: EventCategory = EventCategory::Session;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCheckpointCreated {
    pub session_id: String,
    pub checkpoint_id: String,
    pub message_index: usize,
}

impl Event for SessionCheckpointCreated {
    const NAME: &'static str = "session.checkpoint.created";
    const CATEGORY: EventCategory = EventCategory::Session;
}

// ============================================================================
// MESSAGE EVENTS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageCreated {
    pub session_id: String,
    pub message_id: String,
    pub role: MessageRole,
    pub content: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

impl Event for MessageCreated {
    const NAME: &'static str = "message.created";
    const CATEGORY: EventCategory = EventCategory::Message;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageStreaming {
    pub session_id: String,
    pub message_id: String,
    pub chunk: String,
    pub chunk_index: usize,
    pub is_final: bool,
}

impl Event for MessageStreaming {
    const NAME: &'static str = "message.streaming";
    const CATEGORY: EventCategory = EventCategory::Message;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageCompleted {
    pub session_id: String,
    pub message_id: String,
    pub content: String,
    pub tokens_used: Option<TokenUsage>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
}

impl Event for MessageCompleted {
    const NAME: &'static str = "message.completed";
    const CATEGORY: EventCategory = EventCategory::Message;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageError {
    pub session_id: String,
    pub message_id: Option<String>,
    pub error: String,
    pub error_type: MessageErrorType,
    pub recoverable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageErrorType {
    RateLimit,
    ContextLength,
    InvalidRequest,
    ProviderError,
    NetworkError,
    Unknown,
}

impl Event for MessageError {
    const NAME: &'static str = "message.error";
    const CATEGORY: EventCategory = EventCategory::Message;
}

// ============================================================================
// TOOL EVENTS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequested {
    pub session_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub requires_approval: bool,
}

impl Event for ToolCallRequested {
    const NAME: &'static str = "tool.call.requested";
    const CATEGORY: EventCategory = EventCategory::Tool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallApproved {
    pub session_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub approved_by: ApprovalSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApprovalSource {
    User,
    AutoApprove,
    Policy(String),
}

impl Event for ToolCallApproved {
    const NAME: &'static str = "tool.call.approved";
    const CATEGORY: EventCategory = EventCategory::Tool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRejected {
    pub session_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub reason: String,
}

impl Event for ToolCallRejected {
    const NAME: &'static str = "tool.call.rejected";
    const CATEGORY: EventCategory = EventCategory::Tool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallStarted {
    pub session_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

impl Event for ToolCallStarted {
    const NAME: &'static str = "tool.call.started";
    const CATEGORY: EventCategory = EventCategory::Tool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallCompleted {
    pub session_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub result: ToolResult,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub truncated: bool,
}

impl Event for ToolCallCompleted {
    const NAME: &'static str = "tool.call.completed";
    const CATEGORY: EventCategory = EventCategory::Tool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFailed {
    pub session_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub error: String,
    pub duration_ms: u64,
}

impl Event for ToolCallFailed {
    const NAME: &'static str = "tool.call.failed";
    const CATEGORY: EventCategory = EventCategory::Tool;
}

// ============================================================================
// PROVIDER EVENTS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConnected {
    pub provider_id: String,
    pub provider_name: String,
    pub auth_type: AuthType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthType {
    ApiKey,
    OAuth,
    WellKnown,
}

impl Event for ProviderConnected {
    const NAME: &'static str = "provider.connected";
    const CATEGORY: EventCategory = EventCategory::Provider;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDisconnected {
    pub provider_id: String,
    pub reason: DisconnectReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DisconnectReason {
    UserLogout,
    TokenExpired,
    Error(String),
}

impl Event for ProviderDisconnected {
    const NAME: &'static str = "provider.disconnected";
    const CATEGORY: EventCategory = EventCategory::Provider;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderError {
    pub provider_id: String,
    pub error: String,
    pub error_code: Option<String>,
    pub retryable: bool,
}

impl Event for ProviderError {
    const NAME: &'static str = "provider.error";
    const CATEGORY: EventCategory = EventCategory::Provider;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRateLimited {
    pub provider_id: String,
    pub retry_after_secs: Option<u64>,
    pub limit_type: RateLimitType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RateLimitType {
    Requests,
    Tokens,
    Daily,
}

impl Event for ProviderRateLimited {
    const NAME: &'static str = "provider.rate_limited";
    const CATEGORY: EventCategory = EventCategory::Provider;
}

// ============================================================================
// FILE EVENTS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCreated {
    pub session_id: String,
    pub path: String,
    pub size_bytes: u64,
    pub tool_call_id: Option<String>,
}

impl Event for FileCreated {
    const NAME: &'static str = "file.created";
    const CATEGORY: EventCategory = EventCategory::File;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileModified {
    pub session_id: String,
    pub path: String,
    pub backup_path: String,
    pub change_type: FileChangeType,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileChangeType {
    StrReplace,
    FullRewrite,
    Append,
}

impl Event for FileModified {
    const NAME: &'static str = "file.modified";
    const CATEGORY: EventCategory = EventCategory::File;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDeleted {
    pub session_id: String,
    pub path: String,
    pub backup_path: String,
    pub tool_call_id: Option<String>,
}

impl Event for FileDeleted {
    const NAME: &'static str = "file.deleted";
    const CATEGORY: EventCategory = EventCategory::File;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRestored {
    pub session_id: String,
    pub path: String,
    pub backup_path: String,
    pub restore_reason: RestoreReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RestoreReason {
    Undo,
    Rollback,
    UserRequest,
}

impl Event for FileRestored {
    const NAME: &'static str = "file.restored";
    const CATEGORY: EventCategory = EventCategory::File;
}

// ============================================================================
// SYSTEM EVENTS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStarted {
    pub version: String,
    pub profile: String,
    pub project_root: Option<String>,
}

impl Event for SystemStarted {
    const NAME: &'static str = "system.started";
    const CATEGORY: EventCategory = EventCategory::System;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemShutdown {
    pub reason: ShutdownReason,
    pub session_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShutdownReason {
    UserExit,
    Signal,
    Error(String),
}

impl Event for SystemShutdown {
    const NAME: &'static str = "system.shutdown";
    const CATEGORY: EventCategory = EventCategory::System;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChanged {
    pub key: String,
    pub old_value: Option<String>,
    pub new_value: String,
    pub source: ConfigSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigSource {
    User,
    Profile,
    Environment,
    Default,
}

impl Event for ConfigChanged {
    const NAME: &'static str = "config.changed";
    const CATEGORY: EventCategory = EventCategory::System;
}

// ============================================================================
// USER INTERACTION EVENTS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputReceived {
    pub session_id: String,
    pub input_type: UserInputType,
    pub content_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserInputType {
    Message,
    Command,
    ShellCommand,
    FileAttachment,
}

impl Event for UserInputReceived {
    const NAME: &'static str = "user.input.received";
    const CATEGORY: EventCategory = EventCategory::User;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCancelled {
    pub session_id: String,
    pub cancelled_operation: CancelledOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CancelledOperation {
    Message,
    ToolCall(String),
    Session,
}

impl Event for UserCancelled {
    const NAME: &'static str = "user.cancelled";
    const CATEGORY: EventCategory = EventCategory::User;
}
```

---

### Built-in Event Interceptors

```rust
// libs/shared/src/event_bus/interceptors.rs
use super::*;
use tracing::{debug, info, warn};

/// Logging interceptor - logs all events
pub struct LoggingInterceptor {
    log_level: LogLevel,
}

#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
}

impl LoggingInterceptor {
    pub fn new(log_level: LogLevel) -> Self {
        Self { log_level }
    }
}

impl EventInterceptor for LoggingInterceptor {
    fn on_publish(&self, metadata: &EventMetadata, payload: &serde_json::Value) {
        match self.log_level {
            LogLevel::Debug => debug!(
                event_id = %metadata.id,
                source = %metadata.source,
                correlation_id = ?metadata.correlation_id,
                payload = %payload,
                "Event published"
            ),
            LogLevel::Info => info!(
                event_id = %metadata.id,
                source = %metadata.source,
                "Event published"
            ),
            LogLevel::Warn => {} // Only log warnings/errors
        }
    }
    
    fn on_subscribe(&self, event_name: &str, subscription_id: SubscriptionId) {
        debug!(event = event_name, subscription_id = ?subscription_id, "New subscription");
    }
}

/// Metrics interceptor - tracks event counts and latencies
pub struct MetricsInterceptor {
    event_counts: std::sync::Arc<std::sync::RwLock<HashMap<String, u64>>>,
}

impl MetricsInterceptor {
    pub fn new() -> Self {
        Self {
            event_counts: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }
    
    pub fn get_counts(&self) -> HashMap<String, u64> {
        self.event_counts.read().unwrap().clone()
    }
}

impl EventInterceptor for MetricsInterceptor {
    fn on_publish(&self, metadata: &EventMetadata, _payload: &serde_json::Value) {
        let mut counts = self.event_counts.write().unwrap();
        *counts.entry(metadata.source.clone()).or_insert(0) += 1;
    }
    
    fn on_subscribe(&self, _event_name: &str, _subscription_id: SubscriptionId) {}
}

/// Telemetry interceptor - sends events to telemetry service
pub struct TelemetryInterceptor {
    telemetry_tx: tokio::sync::mpsc::Sender<TelemetryEvent>,
}

#[derive(Debug, Clone)]
pub struct TelemetryEvent {
    pub event_name: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub properties: HashMap<String, serde_json::Value>,
}

impl TelemetryInterceptor {
    pub fn new(telemetry_tx: tokio::sync::mpsc::Sender<TelemetryEvent>) -> Self {
        Self { telemetry_tx }
    }
}

impl EventInterceptor for TelemetryInterceptor {
    fn on_publish(&self, metadata: &EventMetadata, payload: &serde_json::Value) {
        let event = TelemetryEvent {
            event_name: metadata.source.clone(),
            timestamp: metadata.timestamp,
            properties: payload
                .as_object()
                .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default(),
        };
        
        // Non-blocking send
        let _ = self.telemetry_tx.try_send(event);
    }
    
    fn on_subscribe(&self, _event_name: &str, _subscription_id: SubscriptionId) {}
}
```

---

### TUI Integration

```rust
// tui/src/event_handlers.rs
use stakpak_shared::event_bus::{EventBus, events::*};
use std::sync::Arc;

pub struct TuiEventHandlers {
    event_bus: Arc<EventBus>,
}

impl TuiEventHandlers {
    pub async fn setup(event_bus: Arc<EventBus>, app_tx: tokio::sync::mpsc::Sender<AppEvent>) -> Self {
        let handlers = Self { event_bus: event_bus.clone() };
        
        // Subscribe to message streaming for real-time UI updates
        {
            let tx = app_tx.clone();
            event_bus.subscribe_with_handler::<MessageStreaming, _, _>(move |envelope| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(AppEvent::StreamingChunk {
                        message_id: envelope.payload.message_id,
                        chunk: envelope.payload.chunk,
                        is_final: envelope.payload.is_final,
                    }).await;
                }
            }).await.unwrap();
        }
        
        // Subscribe to tool calls for approval popup
        {
            let tx = app_tx.clone();
            event_bus.subscribe_with_handler::<ToolCallRequested, _, _>(move |envelope| {
                let tx = tx.clone();
                async move {
                    if envelope.payload.requires_approval {
                        let _ = tx.send(AppEvent::ShowApprovalPopup {
                            call_id: envelope.payload.call_id,
                            tool_name: envelope.payload.tool_name,
                            arguments: envelope.payload.arguments,
                        }).await;
                    }
                }
            }).await.unwrap();
        }
        
        // Subscribe to tool completion for status updates
        {
            let tx = app_tx.clone();
            event_bus.subscribe_with_handler::<ToolCallCompleted, _, _>(move |envelope| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(AppEvent::ToolCompleted {
                        call_id: envelope.payload.call_id,
                        success: envelope.payload.result.success,
                        duration_ms: envelope.payload.duration_ms,
                    }).await;
                }
            }).await.unwrap();
        }
        
        // Subscribe to errors for notification display
        {
            let tx = app_tx.clone();
            event_bus.subscribe_with_handler::<MessageError, _, _>(move |envelope| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(AppEvent::ShowError {
                        message: envelope.payload.error,
                        recoverable: envelope.payload.recoverable,
                    }).await;
                }
            }).await.unwrap();
        }
        
        // Subscribe to provider rate limits
        {
            let tx = app_tx.clone();
            event_bus.subscribe_with_handler::<ProviderRateLimited, _, _>(move |envelope| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(AppEvent::ShowWarning {
                        message: format!(
                            "Rate limited. Retry in {} seconds.",
                            envelope.payload.retry_after_secs.unwrap_or(60)
                        ),
                    }).await;
                }
            }).await.unwrap();
        }
        
        // Subscribe to file changes for undo tracking
        {
            let tx = app_tx.clone();
            event_bus.subscribe_with_handler::<FileModified, _, _>(move |envelope| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(AppEvent::FileChanged {
                        path: envelope.payload.path,
                        backup_path: envelope.payload.backup_path,
                    }).await;
                }
            }).await.unwrap();
        }
        
        handlers
    }
}

/// Events sent to the TUI app
#[derive(Debug, Clone)]
pub enum AppEvent {
    StreamingChunk {
        message_id: String,
        chunk: String,
        is_final: bool,
    },
    ShowApprovalPopup {
        call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    },
    ToolCompleted {
        call_id: String,
        success: bool,
        duration_ms: u64,
    },
    ShowError {
        message: String,
        recoverable: bool,
    },
    ShowWarning {
        message: String,
    },
    FileChanged {
        path: String,
        backup_path: String,
    },
}
```

---

### Agent Integration

```rust
// cli/src/commands/agent/run/event_publisher.rs
use stakpak_shared::event_bus::{EventBus, EventMetadata, events::*};
use std::sync::Arc;

pub struct AgentEventPublisher {
    event_bus: Arc<EventBus>,
    session_id: String,
}

impl AgentEventPublisher {
    pub fn new(event_bus: Arc<EventBus>, session_id: String) -> Self {
        Self { event_bus, session_id }
    }
    
    pub async fn session_created(&self, name: Option<String>, model: &str) {
        let _ = self.event_bus.publish(SessionCreated {
            session_id: self.session_id.clone(),
            name,
            model: model.to_string(),
            created_at: chrono::Utc::now(),
        }).await;
    }
    
    pub async fn message_streaming(&self, message_id: &str, chunk: &str, chunk_index: usize, is_final: bool) {
        let _ = self.event_bus.publish(MessageStreaming {
            session_id: self.session_id.clone(),
            message_id: message_id.to_string(),
            chunk: chunk.to_string(),
            chunk_index,
            is_final,
        }).await;
    }
    
    pub async fn tool_call_requested(
        &self,
        call_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
        requires_approval: bool,
    ) {
        let _ = self.event_bus.publish(ToolCallRequested {
            session_id: self.session_id.clone(),
            call_id: call_id.to_string(),
            tool_name: tool_name.to_string(),
            arguments,
            requires_approval,
        }).await;
    }
    
    pub async fn tool_call_completed(
        &self,
        call_id: &str,
        tool_name: &str,
        success: bool,
        output: &str,
        duration_ms: u64,
    ) {
        let _ = self.event_bus.publish(ToolCallCompleted {
            session_id: self.session_id.clone(),
            call_id: call_id.to_string(),
            tool_name: tool_name.to_string(),
            result: ToolResult {
                success,
                output: output.to_string(),
                truncated: output.len() > 10000,
            },
            duration_ms,
        }).await;
    }
    
    pub async fn file_modified(&self, path: &str, backup_path: &str, tool_call_id: Option<&str>) {
        let _ = self.event_bus.publish(FileModified {
            session_id: self.session_id.clone(),
            path: path.to_string(),
            backup_path: backup_path.to_string(),
            change_type: FileChangeType::StrReplace,
            tool_call_id: tool_call_id.map(String::from),
        }).await;
    }
    
    pub async fn provider_error(&self, provider_id: &str, error: &str, retryable: bool) {
        let _ = self.event_bus.publish(ProviderError {
            provider_id: provider_id.to_string(),
            error: error.to_string(),
            error_code: None,
            retryable,
        }).await;
    }
}
```

---

### Event-Driven Undo/Redo System

```rust
// libs/shared/src/event_bus/undo_manager.rs
use super::events::*;
use std::collections::VecDeque;

pub struct UndoManager {
    undo_stack: VecDeque<UndoableAction>,
    redo_stack: VecDeque<UndoableAction>,
    max_history: usize,
}

#[derive(Debug, Clone)]
pub enum UndoableAction {
    FileCreated {
        path: String,
    },
    FileModified {
        path: String,
        backup_path: String,
    },
    FileDeleted {
        path: String,
        backup_path: String,
    },
    MessageSent {
        session_id: String,
        message_id: String,
    },
}

impl UndoManager {
    pub fn new(max_history: usize) -> Self {
        Self {
            undo_stack: VecDeque::with_capacity(max_history),
            redo_stack: VecDeque::new(),
            max_history,
        }
    }
    
    /// Subscribe to events that create undoable actions
    pub async fn setup(event_bus: &EventBus) -> Self {
        let manager = Self::new(100);
        
        // This would be connected to the event bus
        // For now, actions are pushed manually
        
        manager
    }
    
    pub fn push(&mut self, action: UndoableAction) {
        if self.undo_stack.len() >= self.max_history {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(action);
        self.redo_stack.clear(); // Clear redo on new action
    }
    
    pub async fn undo(&mut self) -> Option<UndoableAction> {
        if let Some(action) = self.undo_stack.pop_back() {
            // Perform the undo
            match &action {
                UndoableAction::FileCreated { path } => {
                    // Delete the created file
                    let _ = tokio::fs::remove_file(path).await;
                }
                UndoableAction::FileModified { path, backup_path } => {
                    // Restore from backup
                    let _ = tokio::fs::copy(backup_path, path).await;
                }
                UndoableAction::FileDeleted { path, backup_path } => {
                    // Restore the deleted file
                    let _ = tokio::fs::copy(backup_path, path).await;
                }
                UndoableAction::MessageSent { .. } => {
                    // Message undo would need session integration
                }
            }
            
            self.redo_stack.push_back(action.clone());
            Some(action)
        } else {
            None
        }
    }
    
    pub async fn redo(&mut self) -> Option<UndoableAction> {
        if let Some(action) = self.redo_stack.pop_back() {
            // Re-perform the action
            // (Implementation depends on action type)
            
            self.undo_stack.push_back(action.clone());
            Some(action)
        } else {
            None
        }
    }
    
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}
```

---

## Benefits

1. **Decoupling**: Components don't need direct references to each other
2. **Extensibility**: Easy to add new subscribers without modifying publishers
3. **Testing**: Mock event bus for unit tests
4. **Debugging**: Central place to log all events with correlation IDs
5. **Plugin Support**: Plugins can subscribe to events
6. **Replay**: Event history enables debugging and recovery
7. **Metrics**: Interceptors provide observability
8. **Undo/Redo**: Event-driven file change tracking

## Use Cases

| Event | Publishers | Subscribers |
|-------|------------|-------------|
| `session.created` | Agent | TUI, Telemetry, Plugins |
| `message.streaming` | AI Provider | TUI Renderer, Logger |
| `tool.call.requested` | Agent | Approval Popup, Logger, Metrics |
| `tool.call.completed` | Tool Executor | TUI, Undo Manager, Telemetry |
| `file.modified` | File Tools | Undo Manager, File Watcher |
| `provider.error` | Provider | Error Handler, TUI, Retry Logic |
| `provider.rate_limited` | Provider | Backoff Manager, TUI |

## Implementation Effort

| Task | Effort | Priority |
|------|--------|----------|
| Event Bus Core | 2-3 days | High |
| Event Definitions | 1-2 days | High |
| Interceptors | 1 day | Medium |
| TUI Integration | 2-3 days | High |
| Agent Integration | 2-3 days | High |
| Undo Manager | 1-2 days | Medium |
| Testing | 2 days | High |

**Total: ~2 weeks**

## Files to Create/Modify

```
libs/shared/src/
├── event_bus/                # NEW
│   ├── mod.rs               # Core EventBus
│   ├── events.rs            # Event definitions
│   ├── interceptors.rs      # Built-in interceptors
│   └── undo_manager.rs      # Undo/redo system
└── lib.rs                   # MODIFY: export event_bus

tui/src/
├── event_handlers.rs        # NEW: TUI event subscriptions
├── app.rs                   # MODIFY: add event_bus
└── services/
    └── handlers/            # MODIFY: publish events

cli/src/
└── commands/agent/
    └── run/
        ├── event_publisher.rs  # NEW
        └── mod.rs              # MODIFY: use event publisher
```

## Migration Strategy

1. Create event bus infrastructure in `libs/shared`
2. Define core events with proper typing
3. Add logging interceptor for debugging
4. Integrate with TUI (subscribe to events)
5. Integrate with Agent (publish events)
6. Gradually migrate direct calls to events
7. Add metrics interceptor
8. Implement undo manager
9. Document event contracts for plugins

## Testing Strategy

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_publish_subscribe() {
        let bus = EventBus::new();
        
        let mut sub = bus.subscribe::<SessionCreated>().await.unwrap();
        
        bus.publish(SessionCreated {
            session_id: "test".to_string(),
            name: None,
            model: "claude-sonnet-4".to_string(),
            created_at: chrono::Utc::now(),
        }).await.unwrap();
        
        let envelope = sub.recv().await.unwrap();
        assert_eq!(envelope.payload.session_id, "test");
    }
    
    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new();
        
        let mut sub1 = bus.subscribe::<MessageStreaming>().await.unwrap();
        let mut sub2 = bus.subscribe::<MessageStreaming>().await.unwrap();
        
        bus.publish(MessageStreaming {
            session_id: "s1".to_string(),
            message_id: "m1".to_string(),
            chunk: "Hello".to_string(),
            chunk_index: 0,
            is_final: false,
        }).await.unwrap();
        
        // Both subscribers receive the event
        assert!(sub1.recv().await.is_ok());
        assert!(sub2.recv().await.is_ok());
    }
}
```
