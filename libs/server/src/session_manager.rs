use crate::error::SessionManagerError;
use crate::types::{SessionHandle, SessionRuntimeState};
use stakpak_agent_core::AgentCommand;
use std::{collections::HashMap, future::Future, sync::Arc};
use tokio::sync::RwLock;
use uuid::Uuid;

/// In-memory runtime run coordinator (not persistent session storage).
#[derive(Clone, Default)]
pub struct SessionManager {
    states: Arc<RwLock<HashMap<Uuid, SessionRuntimeState>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn state(&self, session_id: Uuid) -> SessionRuntimeState {
        let guard = self.states.read().await;
        guard
            .get(&session_id)
            .cloned()
            .unwrap_or(SessionRuntimeState::Idle)
    }

    pub async fn active_run_id(&self, session_id: Uuid) -> Option<Uuid> {
        self.state(session_id).await.run_id()
    }

    pub async fn running_runs(&self) -> Vec<(Uuid, Uuid)> {
        let guard = self.states.read().await;
        guard
            .iter()
            .filter_map(|(session_id, state)| match state {
                SessionRuntimeState::Running { run_id, .. } => Some((*session_id, *run_id)),
                SessionRuntimeState::Idle
                | SessionRuntimeState::Starting { .. }
                | SessionRuntimeState::Failed { .. } => None,
            })
            .collect()
    }

    pub async fn start_run<F, Fut>(
        &self,
        session_id: Uuid,
        spawn_actor: F,
    ) -> Result<Uuid, SessionManagerError>
    where
        F: FnOnce(Uuid) -> Fut,
        Fut: Future<Output = Result<SessionHandle, String>>,
    {
        let run_id = {
            let mut guard = self.states.write().await;
            match guard.get(&session_id) {
                Some(SessionRuntimeState::Starting { .. })
                | Some(SessionRuntimeState::Running { .. }) => {
                    return Err(SessionManagerError::SessionAlreadyRunning);
                }
                _ => {}
            }

            let run_id = Uuid::new_v4();
            guard.insert(session_id, SessionRuntimeState::Starting { run_id });
            run_id
        };

        match spawn_actor(run_id).await {
            Ok(handle) => {
                let mut guard = self.states.write().await;
                if matches!(
                    guard.get(&session_id),
                    Some(SessionRuntimeState::Starting { run_id: active_run_id })
                        if *active_run_id == run_id
                ) {
                    guard.insert(session_id, SessionRuntimeState::Running { run_id, handle });
                    Ok(run_id)
                } else {
                    let error = "session state changed before actor startup completed".to_string();
                    guard.insert(
                        session_id,
                        SessionRuntimeState::Failed {
                            last_error: error.clone(),
                        },
                    );
                    Err(SessionManagerError::ActorStartupFailed(error))
                }
            }
            Err(error) => {
                let mut guard = self.states.write().await;
                guard.insert(
                    session_id,
                    SessionRuntimeState::Failed {
                        last_error: error.clone(),
                    },
                );
                Err(SessionManagerError::ActorStartupFailed(error))
            }
        }
    }

    pub async fn mark_run_finished(
        &self,
        session_id: Uuid,
        run_id: Uuid,
        outcome: Result<(), String>,
    ) -> Result<(), SessionManagerError> {
        let mut guard = self.states.write().await;

        match guard.get(&session_id) {
            Some(SessionRuntimeState::Starting {
                run_id: active_run_id,
            })
            | Some(SessionRuntimeState::Running {
                run_id: active_run_id,
                ..
            }) => {
                if *active_run_id != run_id {
                    return Err(SessionManagerError::RunMismatch {
                        active_run_id: *active_run_id,
                        requested_run_id: run_id,
                    });
                }
            }
            Some(SessionRuntimeState::Idle) | None | Some(SessionRuntimeState::Failed { .. }) => {
                return Err(SessionManagerError::SessionNotRunning);
            }
        }

        match outcome {
            Ok(()) => {
                guard.insert(session_id, SessionRuntimeState::Idle);
            }
            Err(error) => {
                guard.insert(
                    session_id,
                    SessionRuntimeState::Failed { last_error: error },
                );
            }
        }

        Ok(())
    }

    pub async fn send_command(
        &self,
        session_id: Uuid,
        run_id: Uuid,
        command: AgentCommand,
    ) -> Result<(), SessionManagerError> {
        let command_tx = {
            let guard = self.states.read().await;
            match guard.get(&session_id) {
                Some(SessionRuntimeState::Running {
                    run_id: active_run_id,
                    handle,
                }) => {
                    if *active_run_id != run_id {
                        return Err(SessionManagerError::RunMismatch {
                            active_run_id: *active_run_id,
                            requested_run_id: run_id,
                        });
                    }
                    handle.command_tx.clone()
                }
                Some(SessionRuntimeState::Starting { .. }) => {
                    return Err(SessionManagerError::SessionStarting);
                }
                Some(SessionRuntimeState::Idle)
                | None
                | Some(SessionRuntimeState::Failed { .. }) => {
                    return Err(SessionManagerError::SessionNotRunning);
                }
            }
        };

        command_tx
            .send(command)
            .await
            .map_err(|_| SessionManagerError::CommandChannelClosed)
    }

    pub async fn cancel_run(
        &self,
        session_id: Uuid,
        run_id: Uuid,
    ) -> Result<(), SessionManagerError> {
        let cancel_token = {
            let guard = self.states.read().await;
            match guard.get(&session_id) {
                Some(SessionRuntimeState::Running {
                    run_id: active_run_id,
                    handle,
                }) => {
                    if *active_run_id != run_id {
                        return Err(SessionManagerError::RunMismatch {
                            active_run_id: *active_run_id,
                            requested_run_id: run_id,
                        });
                    }
                    handle.cancel.clone()
                }
                Some(SessionRuntimeState::Starting { .. }) => {
                    return Err(SessionManagerError::SessionStarting);
                }
                Some(SessionRuntimeState::Idle)
                | None
                | Some(SessionRuntimeState::Failed { .. }) => {
                    return Err(SessionManagerError::SessionNotRunning);
                }
            }
        };

        cancel_token.cancel();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stakpak_agent_core::AgentCommand;
    use std::sync::Arc;
    use tokio::{sync::Barrier, sync::mpsc, time::Duration};
    use tokio_util::sync::CancellationToken;

    fn make_handle() -> (SessionHandle, mpsc::Receiver<AgentCommand>) {
        let (command_tx, command_rx) = mpsc::channel(8);
        (
            SessionHandle::new(command_tx, CancellationToken::new()),
            command_rx,
        )
    }

    #[tokio::test]
    async fn start_run_is_atomic_under_concurrency() {
        let manager = Arc::new(SessionManager::new());
        let session_id = Uuid::new_v4();
        let barrier = Arc::new(Barrier::new(2));

        let mut tasks = Vec::new();
        for _ in 0..2 {
            let manager_clone = manager.clone();
            let barrier_clone = barrier.clone();
            let session = session_id;
            tasks.push(tokio::spawn(async move {
                barrier_clone.wait().await;
                manager_clone
                    .start_run(session, |_run_id| async {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        let (handle, _rx) = make_handle();
                        Ok(handle)
                    })
                    .await
            }));
        }

        let mut successes = 0usize;
        let mut conflicts = 0usize;

        for task in tasks {
            match task.await {
                Ok(Ok(_)) => successes += 1,
                Ok(Err(SessionManagerError::SessionAlreadyRunning)) => conflicts += 1,
                Ok(Err(other)) => panic!("unexpected error: {other}"),
                Err(join_error) => panic!("join error: {join_error}"),
            }
        }

        assert_eq!(successes, 1);
        assert_eq!(conflicts, 1);
    }

    #[tokio::test]
    async fn run_scoped_command_rejects_stale_run_id() {
        let manager = SessionManager::new();
        let session_id = Uuid::new_v4();

        let (handle, _rx) = make_handle();
        let run_id = match manager
            .start_run(
                session_id,
                move |_allocated_run_id| async move { Ok(handle) },
            )
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => panic!("start_run should succeed: {error}"),
        };

        let wrong_run_id = Uuid::new_v4();
        let result = manager
            .send_command(session_id, wrong_run_id, AgentCommand::Cancel)
            .await;

        assert_eq!(
            result,
            Err(SessionManagerError::RunMismatch {
                active_run_id: run_id,
                requested_run_id: wrong_run_id,
            })
        );
    }

    #[tokio::test]
    async fn run_scoped_command_accepts_active_run_id() {
        let manager = SessionManager::new();
        let session_id = Uuid::new_v4();

        let (handle, mut rx) = make_handle();
        let run_id = match manager
            .start_run(
                session_id,
                move |_allocated_run_id| async move { Ok(handle) },
            )
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => panic!("start_run should succeed: {error}"),
        };

        let send_result = manager
            .send_command(session_id, run_id, AgentCommand::Cancel)
            .await;
        assert!(send_result.is_ok());

        let received = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
        match received {
            Ok(Some(AgentCommand::Cancel)) => {}
            Ok(Some(_other)) => panic!("unexpected command variant"),
            Ok(None) => panic!("command channel closed unexpectedly"),
            Err(timeout_error) => panic!("did not receive command in time: {timeout_error}"),
        }
    }

    #[tokio::test]
    async fn running_runs_lists_only_running_sessions() {
        let manager = SessionManager::new();
        let running_session_id = Uuid::new_v4();
        let finished_session_id = Uuid::new_v4();

        let (running_handle, _running_rx) = make_handle();
        let running_run_id = match manager
            .start_run(running_session_id, move |_allocated_run_id| async move {
                Ok(running_handle)
            })
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => panic!("start_run should succeed: {error}"),
        };

        let (finished_handle, _finished_rx) = make_handle();
        let finished_run_id = match manager
            .start_run(finished_session_id, move |_allocated_run_id| async move {
                Ok(finished_handle)
            })
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => panic!("start_run should succeed: {error}"),
        };

        let mark_finished = manager
            .mark_run_finished(finished_session_id, finished_run_id, Ok(()))
            .await;
        assert!(mark_finished.is_ok());

        let running_runs = manager.running_runs().await;
        assert_eq!(running_runs.len(), 1);
        assert_eq!(running_runs[0], (running_session_id, running_run_id));
    }

    #[tokio::test]
    async fn startup_failure_transitions_to_failed_state() {
        let manager = SessionManager::new();
        let session_id = Uuid::new_v4();

        let result = manager
            .start_run(session_id, |_run_id| async move { Err("boom".to_string()) })
            .await;

        assert_eq!(
            result,
            Err(SessionManagerError::ActorStartupFailed("boom".to_string()))
        );

        let state = manager.state(session_id).await;
        match state {
            SessionRuntimeState::Failed { last_error } => {
                assert_eq!(last_error, "boom".to_string());
            }
            other => panic!("expected failed state, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn mark_run_finished_requires_active_run_match() {
        let manager = SessionManager::new();
        let session_id = Uuid::new_v4();

        let (handle, _rx) = make_handle();
        let run_id = match manager
            .start_run(
                session_id,
                move |_allocated_run_id| async move { Ok(handle) },
            )
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => panic!("start_run should succeed: {error}"),
        };

        let wrong_run_id = Uuid::new_v4();
        let mismatch = manager
            .mark_run_finished(session_id, wrong_run_id, Ok(()))
            .await;

        assert_eq!(
            mismatch,
            Err(SessionManagerError::RunMismatch {
                active_run_id: run_id,
                requested_run_id: wrong_run_id,
            })
        );

        let finish = manager.mark_run_finished(session_id, run_id, Ok(())).await;
        assert!(finish.is_ok());

        let state = manager.state(session_id).await;
        assert!(matches!(state, SessionRuntimeState::Idle));
    }

    #[tokio::test]
    async fn cancel_run_requires_active_run_match_and_cancels_token() {
        let manager = SessionManager::new();
        let session_id = Uuid::new_v4();

        let (handle, _rx) = make_handle();
        let cancel = handle.cancel.clone();
        let run_id = match manager
            .start_run(
                session_id,
                move |_allocated_run_id| async move { Ok(handle) },
            )
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => panic!("start_run should succeed: {error}"),
        };

        let cancel_result = manager.cancel_run(session_id, run_id).await;
        assert!(cancel_result.is_ok());
        assert!(cancel.is_cancelled());
    }
}
