# Enhancement Proposal: HTTP Server API Layer

## Overview

OpenCode runs an internal HTTP server that exposes a REST API for all operations. This enables IDE integrations, web UIs, and programmatic access. Stakpak currently only has CLI and TUI interfaces.

## Current Stakpak Architecture

```
┌─────────────┐
│    CLI      │──────────────────────────────────────┐
└─────────────┘                                      │
                                                     ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────────┐
│    TUI      │────▶│   Services  │────▶│   AI Providers  │
└─────────────┘     └─────────────┘     └─────────────────┘
```

## OpenCode Server Architecture

```
┌─────────────┐     ┌─────────────┐
│    CLI      │────▶│             │
└─────────────┘     │             │
                    │   HTTP      │     ┌─────────────────┐
┌─────────────┐     │   Server    │────▶│   AI Providers  │
│    TUI      │────▶│  (Hono.js)  │     └─────────────────┘
└─────────────┘     │             │
                    │             │     ┌─────────────────┐
┌─────────────┐     │             │────▶│   MCP Servers   │
│  VS Code    │────▶│             │     └─────────────────┘
└─────────────┘     └─────────────┘
                          │
┌─────────────┐           │
│   Web UI    │───────────┘
└─────────────┘
```

## Proposed Enhancement

### Server Implementation with Axum

```rust
// libs/server/src/lib.rs
use axum::{
    Router,
    routing::{get, post, delete},
    extract::{State, Path, Json},
    response::sse::{Event, Sse},
};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ServerState {
    pub sessions: Arc<RwLock<SessionManager>>,
    pub providers: Arc<ProviderRegistry>,
    pub event_bus: Arc<EventBus>,
}

pub fn create_router(state: ServerState) -> Router {
    Router::new()
        // Session endpoints
        .route("/session", get(list_sessions).post(create_session))
        .route("/session/:id", get(get_session).delete(delete_session))
        .route("/session/:id/message", post(send_message))
        .route("/session/:id/events", get(session_events_sse))
        
        // Provider endpoints
        .route("/provider", get(list_providers))
        .route("/provider/:id/auth", post(auth_provider))
        .route("/provider/:id/oauth/authorize", get(oauth_authorize))
        .route("/provider/:id/oauth/callback", post(oauth_callback))
        
        // MCP endpoints
        .route("/mcp", get(list_mcp_servers))
        .route("/mcp/:name/connect", post(connect_mcp))
        .route("/mcp/:name/disconnect", post(disconnect_mcp))
        
        // Tool endpoints
        .route("/tool", get(list_tools))
        .route("/tool/:name/approve", post(approve_tool))
        
        // Config endpoints
        .route("/config", get(get_config).put(update_config))
        
        .with_state(Arc::new(state))
}
```

### API Endpoints

```rust
// libs/server/src/handlers/session.rs
async fn create_session(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<Session>, ApiError> {
    let session = state.sessions.write().await
        .create(req.name, req.model)
        .await?;
    
    state.event_bus.publish("session.created", &session)?;
    Ok(Json(session))
}

async fn send_message(
    State(state): State<Arc<ServerState>>,
    Path(session_id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<Message>, ApiError> {
    let message = state.sessions.write().await
        .send_message(&session_id, req.content)
        .await?;
    
    Ok(Json(message))
}

// Server-Sent Events for streaming
async fn session_events_sse(
    State(state): State<Arc<ServerState>>,
    Path(session_id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_bus.subscribe_session(&session_id);
    
    let stream = ReceiverStream::new(rx).map(|event| {
        Ok(Event::default()
            .event(event.event_type)
            .data(serde_json::to_string(&event.data).unwrap()))
    });
    
    Sse::new(stream)
}
```

### OpenAPI Schema Generation

```rust
// libs/server/src/schema.rs
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::session::list_sessions,
        handlers::session::create_session,
        handlers::session::get_session,
        handlers::session::send_message,
        handlers::provider::list_providers,
        handlers::provider::auth_provider,
        handlers::mcp::list_mcp_servers,
    ),
    components(schemas(
        Session,
        Message,
        Provider,
        McpServer,
        CreateSessionRequest,
        SendMessageRequest,
    ))
)]
pub struct ApiDoc;
```

### TUI as API Client

```rust
// tui/src/api_client.rs
pub struct ApiClient {
    base_url: String,
    client: reqwest::Client,
}

impl ApiClient {
    pub async fn create_session(&self, name: &str) -> Result<Session> {
        self.client
            .post(&format!("{}/session", self.base_url))
            .json(&CreateSessionRequest { name: name.to_string() })
            .send()
            .await?
            .json()
            .await
    }
    
    pub fn subscribe_events(&self, session_id: &str) -> impl Stream<Item = SessionEvent> {
        let url = format!("{}/session/{}/events", self.base_url, session_id);
        // SSE client implementation
    }
}
```

## Benefits

1. **IDE Integration**: VS Code, JetBrains extensions can use the API
2. **Web UI**: Build a web interface without duplicating logic
3. **Automation**: Scripts can interact with Stakpak programmatically
4. **Testing**: API endpoints are easier to test than TUI
5. **Separation**: Clean boundary between UI and business logic

## API Design Principles

| Principle | Implementation |
|-----------|----------------|
| RESTful | Resource-based URLs, proper HTTP methods |
| Streaming | SSE for real-time updates |
| Typed | OpenAPI schema, generated clients |
| Secure | mTLS, API key auth |
| Versioned | `/v1/` prefix for future compatibility |

## Implementation Effort

| Task | Effort | Priority |
|------|--------|----------|
| Axum Server Setup | 1-2 days | High |
| Session Endpoints | 2-3 days | High |
| Provider Endpoints | 1-2 days | High |
| SSE Streaming | 1-2 days | High |
| OpenAPI Schema | 1 day | Medium |
| TUI API Client | 2-3 days | Medium |
| VS Code Extension | 1-2 weeks | Low |

## Files to Create/Modify

```
libs/
├── server/                    # NEW
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── handlers/
│       │   ├── mod.rs
│       │   ├── session.rs
│       │   ├── provider.rs
│       │   ├── mcp.rs
│       │   └── tool.rs
│       ├── schema.rs
│       └── error.rs

cli/src/
├── commands/
│   └── serve.rs              # NEW: `stakpak serve` command

tui/src/
├── api_client.rs             # NEW
└── app.rs                    # MODIFY: use API client

Cargo.toml                    # MODIFY: add server to workspace
```

## Example Usage

```bash
# Start the server
stakpak serve --port 4096

# Create a session via API
curl -X POST http://localhost:4096/v1/session \
  -H "Content-Type: application/json" \
  -d '{"name": "my-session"}'

# Send a message
curl -X POST http://localhost:4096/v1/session/abc123/message \
  -H "Content-Type: application/json" \
  -d '{"content": "Hello, Stakpak!"}'

# Stream events
curl -N http://localhost:4096/v1/session/abc123/events
```

## Security Considerations

1. **Local Only by Default**: Bind to `127.0.0.1`
2. **mTLS Option**: Use existing cert infrastructure
3. **API Key Auth**: For remote access
4. **Rate Limiting**: Prevent abuse
5. **CORS**: Configurable for web UI
