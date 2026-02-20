use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stakpak_agent_core::AgentEvent;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub id: u64,
    pub session_id: Uuid,
    pub run_id: Option<Uuid>,
    pub timestamp: DateTime<Utc>,
    pub event: AgentEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GapDetected {
    pub requested_after_id: u64,
    pub oldest_available_id: u64,
    pub newest_available_id: u64,
    pub resume_hint: String,
}

pub struct EventSubscription {
    pub replay: Vec<EventEnvelope>,
    pub live: broadcast::Receiver<EventEnvelope>,
    pub gap_detected: Option<GapDetected>,
}

struct SessionEventBuffer {
    next_id: AtomicU64,
    ring: Mutex<VecDeque<EventEnvelope>>,
    tx: broadcast::Sender<EventEnvelope>,
}

impl SessionEventBuffer {
    fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity.max(1) * 2);
        Self {
            next_id: AtomicU64::new(1),
            ring: Mutex::new(VecDeque::with_capacity(capacity.max(1))),
            tx,
        }
    }

    fn lock_ring(&self) -> std::sync::MutexGuard<'_, VecDeque<EventEnvelope>> {
        match self.ring.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[derive(Clone)]
pub struct EventLog {
    capacity: usize,
    sessions: Arc<RwLock<HashMap<Uuid, Arc<SessionEventBuffer>>>>,
}

impl EventLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn publish(
        &self,
        session_id: Uuid,
        run_id: Option<Uuid>,
        event: AgentEvent,
    ) -> EventEnvelope {
        let buffer = self.session_buffer(session_id).await;
        let event_id = buffer.next_id.fetch_add(1, Ordering::SeqCst);

        let envelope = EventEnvelope {
            id: event_id,
            session_id,
            run_id,
            timestamp: Utc::now(),
            event,
        };

        {
            let mut ring = buffer.lock_ring();
            ring.push_back(envelope.clone());

            while ring.len() > self.capacity {
                let _ = ring.pop_front();
            }

            let _ = buffer.tx.send(envelope.clone());
        }

        envelope
    }

    pub async fn subscribe(&self, session_id: Uuid, after_id: Option<u64>) -> EventSubscription {
        let buffer = self.session_buffer(session_id).await;

        let (replay, gap_detected, live) = {
            let ring = buffer.lock_ring();
            let live = buffer.tx.subscribe();

            match after_id {
                None => (Vec::new(), None, live),
                Some(requested_after_id) => {
                    let oldest = ring.front().map(|event| event.id);
                    let newest = ring.back().map(|event| event.id);

                    let (gap, replay) = match (oldest, newest) {
                        (Some(oldest_available_id), Some(newest_available_id))
                            if requested_after_id.saturating_add(1) < oldest_available_id =>
                        {
                            (
                                Some(GapDetected {
                                    requested_after_id,
                                    oldest_available_id,
                                    newest_available_id,
                                    resume_hint: "refresh_snapshot_then_resume".to_string(),
                                }),
                                Vec::new(),
                            )
                        }
                        _ => {
                            let replay = ring
                                .iter()
                                .filter(|event| event.id > requested_after_id)
                                .cloned()
                                .collect();
                            (None, replay)
                        }
                    };

                    (replay, gap, live)
                }
            }
        };

        EventSubscription {
            replay,
            live,
            gap_detected,
        }
    }

    pub async fn snapshot_bounds(&self, session_id: Uuid) -> Option<(u64, u64)> {
        let buffer = self.session_buffer(session_id).await;
        let ring = buffer.lock_ring();

        let oldest = ring.front().map(|event| event.id)?;
        let newest = ring.back().map(|event| event.id)?;

        Some((oldest, newest))
    }

    async fn session_buffer(&self, session_id: Uuid) -> Arc<SessionEventBuffer> {
        {
            let guard = self.sessions.read().await;
            if let Some(existing) = guard.get(&session_id) {
                return existing.clone();
            }
        }

        let mut guard = self.sessions.write().await;
        guard
            .entry(session_id)
            .or_insert_with(|| Arc::new(SessionEventBuffer::new(self.capacity)))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stakpak_agent_core::TurnFinishReason;

    fn run_started(run_id: Uuid) -> AgentEvent {
        AgentEvent::RunStarted { run_id }
    }

    fn turn_completed(run_id: Uuid, turn: usize) -> AgentEvent {
        AgentEvent::TurnCompleted {
            run_id,
            turn,
            finish_reason: TurnFinishReason::Stop,
        }
    }

    #[tokio::test]
    async fn publish_assigns_monotonic_event_ids_per_session() {
        let log = EventLog::new(16);
        let session_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();

        let first = log
            .publish(session_id, Some(run_id), run_started(run_id))
            .await;
        let second = log
            .publish(session_id, Some(run_id), turn_completed(run_id, 1))
            .await;

        assert_eq!(first.id, 1);
        assert_eq!(second.id, 2);
    }

    #[tokio::test]
    async fn replay_returns_events_newer_than_last_event_id() {
        let log = EventLog::new(16);
        let session_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();

        let _ = log
            .publish(session_id, Some(run_id), run_started(run_id))
            .await;
        let second = log
            .publish(session_id, Some(run_id), turn_completed(run_id, 1))
            .await;
        let third = log
            .publish(session_id, Some(run_id), turn_completed(run_id, 2))
            .await;

        let subscription = log.subscribe(session_id, Some(second.id)).await;

        assert!(subscription.gap_detected.is_none());
        assert_eq!(subscription.replay.len(), 1);
        assert_eq!(subscription.replay[0].id, third.id);

        match &subscription.replay[0].event {
            AgentEvent::TurnCompleted { turn, .. } => assert_eq!(*turn, 2),
            other => panic!("expected turn_completed event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn subscribe_reports_gap_when_cursor_falls_outside_ring() {
        let log = EventLog::new(3);
        let session_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();

        for turn in 0..5 {
            let _ = log
                .publish(session_id, Some(run_id), turn_completed(run_id, turn))
                .await;
        }

        let subscription = log.subscribe(session_id, Some(1)).await;

        assert!(subscription.replay.is_empty());

        let gap = match subscription.gap_detected {
            Some(gap) => gap,
            None => panic!("expected gap_detected payload"),
        };

        assert_eq!(gap.requested_after_id, 1);
        assert_eq!(gap.resume_hint, "refresh_snapshot_then_resume".to_string());

        let bounds = log.snapshot_bounds(session_id).await;
        let (oldest, newest) = match bounds {
            Some(bounds) => bounds,
            None => panic!("expected replay bounds for populated session"),
        };

        assert_eq!(gap.oldest_available_id, oldest);
        assert_eq!(gap.newest_available_id, newest);
    }

    #[tokio::test]
    async fn publish_is_durable_without_subscribers() {
        let log = EventLog::new(8);
        let session_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();

        for turn in 0..4 {
            let _ = log
                .publish(session_id, Some(run_id), turn_completed(run_id, turn))
                .await;
        }

        let subscription = log.subscribe(session_id, Some(0)).await;
        assert_eq!(subscription.replay.len(), 4);
    }

    #[tokio::test]
    async fn replay_is_session_scoped() {
        let log = EventLog::new(8);
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();
        let run_a = Uuid::new_v4();
        let run_b = Uuid::new_v4();

        let _ = log
            .publish(session_a, Some(run_a), run_started(run_a))
            .await;
        let _ = log
            .publish(session_b, Some(run_b), run_started(run_b))
            .await;

        let sub_a = log.subscribe(session_a, Some(0)).await;
        let sub_b = log.subscribe(session_b, Some(0)).await;

        assert_eq!(sub_a.replay.len(), 1);
        assert_eq!(sub_b.replay.len(), 1);
        assert_eq!(sub_a.replay[0].session_id, session_a);
        assert_eq!(sub_b.replay[0].session_id, session_b);
    }
}
