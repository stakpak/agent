//! Cron scheduler integration for watch triggers.
//!
//! Uses tokio-cron-scheduler to schedule and execute triggers based on cron expressions.
//! Users provide standard 5-part cron expressions (min hour day month weekday),
//! which are converted internally to 6-part format (with seconds) for the scheduler.

use tokio::sync::mpsc;
use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};
use tracing::{debug, error, info};
use uuid::Uuid;

use super::config::Trigger;

/// Convert a standard 5-part cron expression to 6-part by prepending "0 " for seconds.
/// If already 6-part, returns as-is.
///
/// 5-part: "min hour day month weekday" (standard cron)
/// 6-part: "sec min hour day month weekday" (tokio-cron-scheduler internal)
fn to_six_part_cron(expr: &str) -> String {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() == 5 {
        format!("0 {}", expr)
    } else {
        expr.to_string()
    }
}

/// Message sent when a trigger fires.
#[derive(Debug, Clone)]
pub struct TriggerEvent {
    /// Name of the trigger that fired.
    pub trigger_name: String,
    /// The trigger configuration.
    pub trigger: Trigger,
}

/// Scheduler errors.
#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("Failed to create scheduler: {0}")]
    CreateError(String),

    #[error("Failed to add job: {0}")]
    AddJobError(String),

    #[error("Failed to start scheduler: {0}")]
    StartError(String),

    #[error("Failed to shutdown scheduler: {0}")]
    ShutdownError(String),

    #[error("Invalid cron expression '{expression}': {message}")]
    InvalidCron { expression: String, message: String },
}

impl From<JobSchedulerError> for SchedulerError {
    fn from(err: JobSchedulerError) -> Self {
        SchedulerError::CreateError(err.to_string())
    }
}

/// Watch scheduler that manages trigger jobs.
pub struct WatchScheduler {
    scheduler: JobScheduler,
    /// Channel to send trigger events when jobs fire.
    event_tx: mpsc::Sender<TriggerEvent>,
    /// Registered job IDs for cleanup.
    job_ids: Vec<Uuid>,
}

impl WatchScheduler {
    /// Create a new scheduler with an event channel.
    ///
    /// Returns the scheduler and a receiver for trigger events.
    pub async fn new() -> Result<(Self, mpsc::Receiver<TriggerEvent>), SchedulerError> {
        let scheduler = JobScheduler::new()
            .await
            .map_err(|e| SchedulerError::CreateError(e.to_string()))?;

        // Channel for trigger events - buffer up to 100 events
        let (event_tx, event_rx) = mpsc::channel(100);

        Ok((
            Self {
                scheduler,
                event_tx,
                job_ids: Vec::new(),
            },
            event_rx,
        ))
    }

    /// Register a trigger with the scheduler.
    pub async fn register_trigger(&mut self, trigger: Trigger) -> Result<Uuid, SchedulerError> {
        let trigger_name = trigger.name.clone();
        let schedule = trigger.schedule.clone();
        let schedule_6part = to_six_part_cron(&schedule);
        let event_tx = self.event_tx.clone();
        let trigger_clone = trigger.clone();

        info!(
            trigger = %trigger_name,
            schedule = %schedule,
            "Registering trigger with scheduler"
        );

        // Create the job with the 6-part cron schedule
        let job = Job::new_async(schedule_6part.as_str(), move |_uuid, _lock| {
            let trigger_name = trigger_name.clone();
            let trigger = trigger_clone.clone();
            let tx = event_tx.clone();

            Box::pin(async move {
                debug!(trigger = %trigger_name, "Trigger fired");

                let event = TriggerEvent {
                    trigger_name: trigger_name.clone(),
                    trigger,
                };

                if let Err(e) = tx.send(event).await {
                    error!(
                        trigger = %trigger_name,
                        error = %e,
                        "Failed to send trigger event"
                    );
                }
            })
        })
        .map_err(|e| SchedulerError::InvalidCron {
            expression: schedule,
            message: e.to_string(),
        })?;

        let job_id = job.guid();

        self.scheduler
            .add(job)
            .await
            .map_err(|e| SchedulerError::AddJobError(e.to_string()))?;

        self.job_ids.push(job_id);

        Ok(job_id)
    }

    /// Register multiple triggers.
    pub async fn register_triggers(
        &mut self,
        triggers: Vec<Trigger>,
    ) -> Result<Vec<Uuid>, SchedulerError> {
        let mut job_ids = Vec::new();

        for trigger in triggers {
            let job_id = self.register_trigger(trigger).await?;
            job_ids.push(job_id);
        }

        Ok(job_ids)
    }

    /// Start the scheduler.
    pub async fn start(&self) -> Result<(), SchedulerError> {
        info!("Starting watch scheduler");

        self.scheduler
            .start()
            .await
            .map_err(|e| SchedulerError::StartError(e.to_string()))?;

        Ok(())
    }

    /// Shutdown the scheduler gracefully.
    pub async fn shutdown(&mut self) -> Result<(), SchedulerError> {
        info!("Shutting down watch scheduler");

        self.scheduler
            .shutdown()
            .await
            .map_err(|e| SchedulerError::ShutdownError(e.to_string()))?;

        Ok(())
    }

    /// Get the number of registered jobs.
    pub fn job_count(&self) -> usize {
        self.job_ids.len()
    }

    /// Get the registered job IDs.
    pub fn job_ids(&self) -> &[Uuid] {
        &self.job_ids
    }

    /// Get the next scheduled time for a job.
    pub async fn next_tick_for_job(
        &mut self,
        job_id: Uuid,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, SchedulerError> {
        self.scheduler
            .next_tick_for_job(job_id)
            .await
            .map_err(|e| SchedulerError::CreateError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    fn create_test_trigger(name: &str, schedule: &str) -> Trigger {
        Trigger {
            name: name.to_string(),
            schedule: schedule.to_string(),
            check: None,
            check_timeout: None,
            check_trigger_on: None,
            prompt: "Test prompt".to_string(),
            profile: None,
            board_id: None,
            timeout: None,
            enable_slack_tools: None,
            enable_subagents: None,
            pause_on_approval: None,
            notify_on: None,
            notify_channel: None,
            notify_chat_id: None,
        }
    }

    #[tokio::test]
    async fn test_scheduler_creation() {
        let result = WatchScheduler::new().await;
        assert!(result.is_ok());

        let (scheduler, _rx) = result.unwrap();
        assert_eq!(scheduler.job_count(), 0);
    }

    #[tokio::test]
    async fn test_register_trigger() {
        let (mut scheduler, _rx) = WatchScheduler::new()
            .await
            .expect("Failed to create scheduler");

        // Standard 5-part cron expression (converted internally to 6-part)
        let trigger = create_test_trigger("test-trigger", "0 * * * *");
        let result = scheduler.register_trigger(trigger).await;

        assert!(result.is_ok());
        assert_eq!(scheduler.job_count(), 1);
    }

    #[tokio::test]
    async fn test_register_multiple_triggers() {
        let (mut scheduler, _rx) = WatchScheduler::new()
            .await
            .expect("Failed to create scheduler");

        // Standard 5-part cron expressions
        let triggers = vec![
            create_test_trigger("trigger-1", "0 * * * *"), // Every hour
            create_test_trigger("trigger-2", "*/5 * * * *"), // Every 5 minutes
            create_test_trigger("trigger-3", "0 0 * * *"), // Daily at midnight
        ];

        let result = scheduler.register_triggers(triggers).await;

        assert!(result.is_ok());
        assert_eq!(scheduler.job_count(), 3);
    }

    #[tokio::test]
    async fn test_invalid_cron_expression() {
        let (mut scheduler, _rx) = WatchScheduler::new()
            .await
            .expect("Failed to create scheduler");

        let trigger = create_test_trigger("bad-trigger", "invalid cron");
        let result = scheduler.register_trigger(trigger).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SchedulerError::InvalidCron { .. }
        ));
    }

    #[tokio::test]
    async fn test_scheduler_start_and_shutdown() {
        let (mut scheduler, _rx) = WatchScheduler::new()
            .await
            .expect("Failed to create scheduler");

        // Standard 5-part cron expression
        let trigger = create_test_trigger("test-trigger", "0 * * * *");
        scheduler
            .register_trigger(trigger)
            .await
            .expect("Failed to register trigger");

        // Start the scheduler
        let start_result = scheduler.start().await;
        assert!(start_result.is_ok());

        // Give it a moment to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Shutdown
        let shutdown_result = scheduler.shutdown().await;
        assert!(shutdown_result.is_ok());
    }

    #[tokio::test]
    async fn test_job_execution() {
        let (mut scheduler, mut rx) = WatchScheduler::new()
            .await
            .expect("Failed to create scheduler");

        // Use a very frequent schedule - must use 6-part here since 5-part can't express "every second"
        // This is the only case where 6-part is needed (sub-minute scheduling)
        let trigger = create_test_trigger("fast-trigger", "* * * * * *");
        scheduler
            .register_trigger(trigger)
            .await
            .expect("Failed to register trigger");

        // Start the scheduler
        scheduler.start().await.expect("Failed to start scheduler");

        // Wait for an event (with timeout)
        let event_result = timeout(Duration::from_secs(3), rx.recv()).await;

        // Shutdown
        scheduler.shutdown().await.expect("Failed to shutdown");

        // Verify we received an event
        assert!(event_result.is_ok(), "Timed out waiting for trigger event");
        let event = event_result.unwrap();
        assert!(event.is_some(), "Channel closed without receiving event");

        let event = event.unwrap();
        assert_eq!(event.trigger_name, "fast-trigger");
    }

    #[tokio::test]
    async fn test_various_cron_expressions() {
        let (mut scheduler, _rx) = WatchScheduler::new()
            .await
            .expect("Failed to create scheduler");

        // Standard 5-part cron expressions (converted internally)
        let expressions = [
            ("every-minute", "* * * * *"),      // Every minute
            ("every-5-minutes", "*/5 * * * *"), // Every 5 minutes
            ("hourly", "0 * * * *"),            // Every hour at :00
            ("daily-midnight", "0 0 * * *"),    // Daily at midnight
            ("weekly-sunday", "0 0 * * 0"),     // Weekly on Sunday at midnight
            ("monthly", "0 0 1 * *"),           // Monthly on 1st at midnight
            ("weekdays-9am", "0 9 * * 1-5"),    // Weekdays at 9 AM
        ];

        for (name, schedule) in expressions {
            let trigger = create_test_trigger(name, schedule);
            let result = scheduler.register_trigger(trigger).await;
            assert!(
                result.is_ok(),
                "Failed to register trigger with schedule '{}': {:?}",
                schedule,
                result.err()
            );
        }

        assert_eq!(scheduler.job_count(), expressions.len());
    }

    #[tokio::test]
    async fn test_next_tick_for_job() {
        let (mut scheduler, _rx) = WatchScheduler::new()
            .await
            .expect("Failed to create scheduler");

        // Standard 5-part cron expression
        let trigger = create_test_trigger("test-trigger", "0 * * * *");
        let job_id = scheduler
            .register_trigger(trigger)
            .await
            .expect("Failed to register trigger");

        scheduler.start().await.expect("Failed to start scheduler");

        let next_tick = scheduler.next_tick_for_job(job_id).await;
        assert!(next_tick.is_ok());

        // The next tick should be in the future
        if let Ok(Some(tick)) = next_tick {
            assert!(tick > chrono::Utc::now());
        }

        scheduler.shutdown().await.expect("Failed to shutdown");
    }

    #[test]
    fn test_to_six_part_cron() {
        // 5-part should get "0 " prepended
        assert_eq!(to_six_part_cron("* * * * *"), "0 * * * * *");
        assert_eq!(to_six_part_cron("0 9 * * 1-5"), "0 0 9 * * 1-5");
        assert_eq!(to_six_part_cron("*/5 * * * *"), "0 */5 * * * *");

        // 6-part should pass through unchanged
        assert_eq!(to_six_part_cron("* * * * * *"), "* * * * * *");
        assert_eq!(to_six_part_cron("0 0 9 * * 1-5"), "0 0 9 * * 1-5");

        // Edge cases (invalid, but let scheduler validate)
        assert_eq!(to_six_part_cron("invalid"), "invalid");
        assert_eq!(to_six_part_cron("* * *"), "* * *");
    }
}
