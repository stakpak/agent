//! Stakpak API storage implementation
//!
//! Implements SessionStorage using Stakpak's /v1/sessions API.

use crate::stakpak::{self as stakpak_api, StakpakApiClient, StakpakApiConfig};
use crate::storage::{
    BackendInfo, Checkpoint, CheckpointState, CheckpointSummary, CreateCheckpointRequest,
    CreateSessionRequest, CreateSessionResult, ListCheckpointsQuery, ListCheckpointsResult,
    ListSessionsQuery, ListSessionsResult, Session, SessionStatus, SessionStorage, SessionSummary,
    SessionVisibility, StorageError, UpdateSessionRequest,
};
use async_trait::async_trait;
use uuid::Uuid;

/// Stakpak API storage implementation
#[derive(Clone)]
pub struct StakpakStorage {
    client: StakpakApiClient,
    backend_info: BackendInfo,
}

impl StakpakStorage {
    /// Create a new Stakpak storage client
    pub fn new(api_key: &str, api_endpoint: &str) -> Result<Self, StorageError> {
        Self::new_with_profile(api_key, api_endpoint, None)
    }

    pub fn new_with_profile(
        api_key: &str,
        api_endpoint: &str,
        profile: Option<String>,
    ) -> Result<Self, StorageError> {
        let client = StakpakApiClient::new(&StakpakApiConfig {
            api_key: api_key.to_string(),
            api_endpoint: api_endpoint.to_string(),
            rulebook_base_url: None,
        })
        .map_err(StorageError::Connection)?;

        Ok(Self {
            client,
            backend_info: BackendInfo::stakpak_api(profile, api_endpoint.to_string()),
        })
    }

    /// Get the underlying API client
    pub fn client(&self) -> &StakpakApiClient {
        &self.client
    }
}

#[async_trait]
impl SessionStorage for StakpakStorage {
    fn backend_info(&self) -> BackendInfo {
        self.backend_info.clone()
    }

    async fn list_sessions(
        &self,
        query: &ListSessionsQuery,
    ) -> Result<ListSessionsResult, StorageError> {
        let api_query = stakpak_api::ListSessionsQuery {
            limit: query.limit,
            offset: query.offset,
            search: query.search.clone(),
            status: query.status.map(|s| match s {
                SessionStatus::Active => "ACTIVE".to_string(),
                SessionStatus::Deleted => "DELETED".to_string(),
            }),
            visibility: query.visibility.map(|v| match v {
                SessionVisibility::Private => "PRIVATE".to_string(),
                SessionVisibility::Public => "PUBLIC".to_string(),
            }),
        };

        let response = self
            .client
            .list_sessions(&api_query)
            .await
            .map_err(map_api_error)?;

        Ok(ListSessionsResult {
            sessions: response
                .sessions
                .into_iter()
                .map(|s| SessionSummary {
                    id: s.id,
                    title: s.title,
                    visibility: match s.visibility {
                        stakpak_api::SessionVisibility::Private => SessionVisibility::Private,
                        stakpak_api::SessionVisibility::Public => SessionVisibility::Public,
                    },
                    status: match s.status {
                        stakpak_api::SessionStatus::Active => SessionStatus::Active,
                        stakpak_api::SessionStatus::Deleted => SessionStatus::Deleted,
                    },
                    cwd: s.cwd,
                    created_at: s.created_at,
                    updated_at: s.updated_at,
                    message_count: s.message_count,
                    active_checkpoint_id: Some(s.active_checkpoint_id),
                    last_message_at: s.last_message_at,
                })
                .collect(),
            total: None,
        })
    }

    async fn get_session(&self, session_id: Uuid) -> Result<Session, StorageError> {
        let response = self
            .client
            .get_session(session_id)
            .await
            .map_err(map_api_error)?;
        let s = response.session;

        Ok(Session {
            id: s.id,
            title: s.title,
            visibility: match s.visibility {
                stakpak_api::SessionVisibility::Private => SessionVisibility::Private,
                stakpak_api::SessionVisibility::Public => SessionVisibility::Public,
            },
            status: match s.status {
                stakpak_api::SessionStatus::Active => SessionStatus::Active,
                stakpak_api::SessionStatus::Deleted => SessionStatus::Deleted,
            },
            cwd: s.cwd,
            created_at: s.created_at,
            updated_at: s.updated_at,
            active_checkpoint: s.active_checkpoint.map(|c| Checkpoint {
                id: c.id,
                session_id: c.session_id,
                parent_id: c.parent_id,
                state: CheckpointState {
                    messages: c.state.messages,
                    metadata: c.state.metadata,
                },
                created_at: c.created_at,
                updated_at: c.updated_at,
            }),
        })
    }

    async fn create_session(
        &self,
        request: &CreateSessionRequest,
    ) -> Result<CreateSessionResult, StorageError> {
        let api_request = stakpak_api::CreateSessionRequest {
            title: request.title.clone(),
            visibility: Some(match request.visibility {
                SessionVisibility::Private => stakpak_api::SessionVisibility::Private,
                SessionVisibility::Public => stakpak_api::SessionVisibility::Public,
            }),
            cwd: request.cwd.clone(),
            state: stakpak_api::CheckpointState {
                messages: request.initial_state.messages.clone(),
                metadata: request.initial_state.metadata.clone(),
            },
        };

        let response = self
            .client
            .create_session(&api_request)
            .await
            .map_err(map_api_error)?;

        Ok(CreateSessionResult {
            session_id: response.session_id,
            checkpoint: Checkpoint {
                id: response.checkpoint.id,
                session_id: response.checkpoint.session_id,
                parent_id: response.checkpoint.parent_id,
                state: CheckpointState {
                    messages: response.checkpoint.state.messages,
                    metadata: response.checkpoint.state.metadata,
                },
                created_at: response.checkpoint.created_at,
                updated_at: response.checkpoint.updated_at,
            },
        })
    }

    async fn update_session(
        &self,
        session_id: Uuid,
        request: &UpdateSessionRequest,
    ) -> Result<Session, StorageError> {
        let api_request = stakpak_api::UpdateSessionRequest {
            title: request.title.clone(),
            visibility: request.visibility.map(|v| match v {
                SessionVisibility::Private => stakpak_api::SessionVisibility::Private,
                SessionVisibility::Public => stakpak_api::SessionVisibility::Public,
            }),
        };

        let response = self
            .client
            .update_session(session_id, &api_request)
            .await
            .map_err(map_api_error)?;
        let s = response.session;

        Ok(Session {
            id: s.id,
            title: s.title,
            visibility: match s.visibility {
                stakpak_api::SessionVisibility::Private => SessionVisibility::Private,
                stakpak_api::SessionVisibility::Public => SessionVisibility::Public,
            },
            status: match s.status {
                stakpak_api::SessionStatus::Active => SessionStatus::Active,
                stakpak_api::SessionStatus::Deleted => SessionStatus::Deleted,
            },
            cwd: s.cwd,
            created_at: s.created_at,
            updated_at: s.updated_at,
            active_checkpoint: s.active_checkpoint.map(|c| Checkpoint {
                id: c.id,
                session_id: c.session_id,
                parent_id: c.parent_id,
                state: CheckpointState {
                    messages: c.state.messages,
                    metadata: c.state.metadata,
                },
                created_at: c.created_at,
                updated_at: c.updated_at,
            }),
        })
    }

    async fn delete_session(&self, session_id: Uuid) -> Result<(), StorageError> {
        self.client
            .delete_session(session_id)
            .await
            .map_err(map_api_error)
    }

    async fn list_checkpoints(
        &self,
        session_id: Uuid,
        query: &ListCheckpointsQuery,
    ) -> Result<ListCheckpointsResult, StorageError> {
        let api_query = stakpak_api::ListCheckpointsQuery {
            limit: query.limit,
            offset: query.offset,
            include_state: query.include_state,
        };

        let response = self
            .client
            .list_checkpoints(session_id, &api_query)
            .await
            .map_err(map_api_error)?;

        Ok(ListCheckpointsResult {
            checkpoints: response
                .checkpoints
                .into_iter()
                .map(|c| CheckpointSummary {
                    id: c.id,
                    session_id: c.session_id,
                    parent_id: c.parent_id,
                    message_count: c.message_count,
                    created_at: c.created_at,
                    updated_at: c.updated_at,
                })
                .collect(),
            total: None,
        })
    }

    async fn get_checkpoint(&self, checkpoint_id: Uuid) -> Result<Checkpoint, StorageError> {
        let response = self
            .client
            .get_checkpoint(checkpoint_id)
            .await
            .map_err(map_api_error)?;
        let c = response.checkpoint;

        Ok(Checkpoint {
            id: c.id,
            session_id: c.session_id,
            parent_id: c.parent_id,
            state: CheckpointState {
                messages: c.state.messages,
                metadata: c.state.metadata,
            },
            created_at: c.created_at,
            updated_at: c.updated_at,
        })
    }

    async fn create_checkpoint(
        &self,
        session_id: Uuid,
        request: &CreateCheckpointRequest,
    ) -> Result<Checkpoint, StorageError> {
        let api_request = stakpak_api::CreateCheckpointRequest {
            state: stakpak_api::CheckpointState {
                messages: request.state.messages.clone(),
                metadata: request.state.metadata.clone(),
            },
            parent_id: request.parent_id,
        };

        let response = self
            .client
            .create_checkpoint(session_id, &api_request)
            .await
            .map_err(map_api_error)?;

        Ok(Checkpoint {
            id: response.checkpoint.id,
            session_id: response.checkpoint.session_id,
            parent_id: response.checkpoint.parent_id,
            state: CheckpointState {
                messages: response.checkpoint.state.messages,
                metadata: response.checkpoint.state.metadata,
            },
            created_at: response.checkpoint.created_at,
            updated_at: response.checkpoint.updated_at,
        })
    }
}

/// Map API error strings to StorageError
fn map_api_error(error: String) -> StorageError {
    if error.contains("not found") || error.contains("Not found") {
        StorageError::NotFound(error)
    } else if error.contains("unauthorized")
        || error.contains("Unauthorized")
        || error.contains("401")
    {
        StorageError::Unauthorized(error)
    } else if error.contains("rate limit") || error.contains("Rate limit") || error.contains("429")
    {
        StorageError::RateLimited(error)
    } else if error.contains("invalid") || error.contains("Invalid") || error.contains("400") {
        StorageError::InvalidRequest(error)
    } else {
        StorageError::Internal(error)
    }
}

#[cfg(test)]
mod tests {
    use axum::{Json, Router, extract::Path, routing::get};
    use chrono::Utc;
    use serde_json::json;
    use tokio::net::TcpListener;

    use super::*;

    #[tokio::test]
    async fn listed_remote_session_id_is_fetchable_via_get_session() {
        let session_id = Uuid::new_v4();
        let checkpoint_id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let list_body = json!({
            "sessions": [
                {
                    "id": session_id,
                    "title": "Round Trip",
                    "visibility": "PRIVATE",
                    "status": "ACTIVE",
                    "cwd": "/tmp/project",
                    "created_at": now,
                    "updated_at": now,
                    "message_count": 1,
                    "active_checkpoint_id": checkpoint_id,
                    "last_message_at": now
                }
            ]
        });
        let show_body = json!({
            "session": {
                "id": session_id,
                "title": "Round Trip",
                "visibility": "PRIVATE",
                "status": "ACTIVE",
                "cwd": "/tmp/project",
                "created_at": now,
                "updated_at": now,
                "deleted_at": null,
                "active_checkpoint": {
                    "id": checkpoint_id,
                    "session_id": session_id,
                    "parent_id": null,
                    "state": {
                        "messages": [
                            {
                                "role": "user",
                                "content": "hi"
                            }
                        ],
                        "metadata": null
                    },
                    "created_at": now,
                    "updated_at": now
                }
            }
        });

        let app = Router::new()
            .route(
                "/v1/sessions",
                get({
                    let list_body = list_body.clone();
                    move || {
                        let list_body = list_body.clone();
                        async move { Json(list_body) }
                    }
                }),
            )
            .route(
                "/v1/sessions/{id}",
                get({
                    let show_body = show_body.clone();
                    move |Path(id): Path<Uuid>| {
                        let show_body = show_body.clone();
                        async move {
                            assert_eq!(id, session_id, "show should request listed session id");
                            Json(show_body)
                        }
                    }
                }),
            );

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let storage = StakpakStorage::new("test-key", &format!("http://{addr}"))
            .expect("storage should build");
        let listed = storage
            .list_sessions(&ListSessionsQuery::new().with_limit(10))
            .await
            .expect("list sessions should succeed");
        let first_id = listed.sessions.first().expect("session from list").id;
        let fetched = storage
            .get_session(first_id)
            .await
            .expect("get_session should accept listed id");

        assert_eq!(fetched.id, session_id);
        assert_eq!(fetched.title, "Round Trip");
        assert_eq!(fetched.cwd.as_deref(), Some("/tmp/project"));

        server.abort();
        if let Err(join_err) = server.await
            && !join_err.is_cancelled()
        {
            panic!("server task failed: {join_err}");
        }
    }
}
