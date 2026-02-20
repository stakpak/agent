use super::config::Schedule;
use super::scheduler::Scheduler;
use std::collections::HashMap;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Snapshot of currently registered cron jobs keyed by schedule name.
#[derive(Debug, Clone, Default)]
pub struct ScheduleSnapshot {
    pub registered: HashMap<String, RegisteredSchedule>,
}

#[derive(Debug, Clone)]
pub struct RegisteredSchedule {
    pub cron: String,
    pub job_id: Uuid,
}

/// Diff running cron jobs against the new schedule config and converge.
///
/// - Enabled schedules in config but not registered -> add
/// - Registered schedules missing/disabled in config -> remove
/// - Schedules with changed cron -> remove old job then add new
/// - Schedules unchanged -> keep existing job
///
/// The returned snapshot always reflects the actual scheduler state as best as possible.
pub async fn reconcile_schedules(
    scheduler: &mut Scheduler,
    current: &ScheduleSnapshot,
    new_schedules: &[Schedule],
) -> ScheduleSnapshot {
    let desired: Vec<&Schedule> = new_schedules
        .iter()
        .filter(|schedule| schedule.enabled)
        .collect();

    let desired_map: HashMap<&str, &Schedule> = desired
        .iter()
        .map(|schedule| (schedule.name.as_str(), *schedule))
        .collect();

    let mut next = HashMap::new();
    let mut added = 0u32;
    let mut removed = 0u32;
    let mut updated = 0u32;
    let mut retained_count = 0u32;
    let mut rollback_count = 0u32;

    // Remove schedules deleted from config or newly disabled.
    for (name, registered) in &current.registered {
        if !desired_map.contains_key(name.as_str()) {
            info!(schedule = %name, "Removing schedule (deleted or disabled)");
            if let Err(error) = scheduler.remove_job(registered.job_id).await {
                warn!(
                    schedule = %name,
                    error = %error,
                    "Failed to remove job; keeping snapshot entry"
                );
                next.insert(name.clone(), registered.clone());
                retained_count += 1;
            } else {
                removed += 1;
            }
        }
    }

    // Add new schedules or update changed crons.
    for schedule in desired {
        if let Some(existing) = current.registered.get(&schedule.name) {
            if next.contains_key(&schedule.name) {
                // Kept due to earlier remove failure.
                continue;
            }

            if existing.cron == schedule.cron {
                next.insert(schedule.name.clone(), existing.clone());
                continue;
            }

            info!(
                schedule = %schedule.name,
                old_cron = %existing.cron,
                new_cron = %schedule.cron,
                "Updating schedule (cron changed)"
            );

            if let Err(error) = scheduler.remove_job(existing.job_id).await {
                warn!(
                    schedule = %schedule.name,
                    error = %error,
                    "Failed to remove old job; keeping existing schedule"
                );
                next.insert(schedule.name.clone(), existing.clone());
                retained_count += 1;
                continue;
            }

            match scheduler.register_schedule(schedule.clone()).await {
                Ok(job_id) => {
                    updated += 1;
                    next.insert(
                        schedule.name.clone(),
                        RegisteredSchedule {
                            cron: schedule.cron.clone(),
                            job_id,
                        },
                    );
                }
                Err(error) => {
                    error!(
                        schedule = %schedule.name,
                        error = %error,
                        "Failed to register updated schedule; attempting rollback"
                    );

                    let mut rollback_schedule = schedule.clone();
                    rollback_schedule.cron = existing.cron.clone();
                    match scheduler.register_schedule(rollback_schedule).await {
                        Ok(job_id) => {
                            warn!(
                                schedule = %schedule.name,
                                "Rollback succeeded with previous cron schedule"
                            );
                            next.insert(
                                schedule.name.clone(),
                                RegisteredSchedule {
                                    cron: existing.cron.clone(),
                                    job_id,
                                },
                            );
                            rollback_count += 1;
                        }
                        Err(rollback_error) => {
                            error!(
                                schedule = %schedule.name,
                                error = %rollback_error,
                                "Rollback failed; schedule remains unregistered"
                            );
                        }
                    }
                }
            }
            continue;
        }

        info!(schedule = %schedule.name, cron = %schedule.cron, "Adding new schedule");
        match scheduler.register_schedule(schedule.clone()).await {
            Ok(job_id) => {
                next.insert(
                    schedule.name.clone(),
                    RegisteredSchedule {
                        cron: schedule.cron.clone(),
                        job_id,
                    },
                );
                added += 1;
            }
            Err(error) => {
                error!(schedule = %schedule.name, error = %error, "Failed to register schedule");
            }
        }
    }

    info!(
        total = next.len(),
        added, removed, updated, retained_count, rollback_count, "Schedule reconciliation complete"
    );

    ScheduleSnapshot { registered: next }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(name: &str, cron: &str) -> Schedule {
        Schedule {
            name: name.to_string(),
            cron: cron.to_string(),
            check: None,
            check_timeout: None,
            trigger_on: None,
            prompt: "test".to_string(),
            profile: None,
            board_id: None,
            timeout: None,
            enable_slack_tools: None,
            enable_subagents: None,
            pause_on_approval: None,
            sandbox: None,
            notify_on: None,
            notify_channel: None,
            notify_chat_id: None,
            enabled: true,
        }
    }

    fn disabled_schedule(name: &str, cron: &str) -> Schedule {
        let mut value = schedule(name, cron);
        value.enabled = false;
        value
    }

    #[tokio::test]
    async fn test_reconcile_add_new_schedule() {
        let (mut scheduler, _rx) = Scheduler::new().await.expect("scheduler should initialize");
        let current = ScheduleSnapshot::default();

        let next =
            reconcile_schedules(&mut scheduler, &current, &[schedule("a", "*/5 * * * *")]).await;

        assert_eq!(next.registered.len(), 1);
    }

    #[tokio::test]
    async fn test_reconcile_no_change_keeps_job_id() {
        let (mut scheduler, _rx) = Scheduler::new().await.expect("scheduler should initialize");
        let first = reconcile_schedules(
            &mut scheduler,
            &ScheduleSnapshot::default(),
            &[schedule("a", "*/5 * * * *")],
        )
        .await;

        let second =
            reconcile_schedules(&mut scheduler, &first, &[schedule("a", "*/5 * * * *")]).await;

        let first_id = first
            .registered
            .get("a")
            .expect("first snapshot should contain schedule a")
            .job_id;
        let second_id = second
            .registered
            .get("a")
            .expect("second snapshot should contain schedule a")
            .job_id;

        assert_eq!(first_id, second_id);
    }

    #[tokio::test]
    async fn test_reconcile_update_cron_changes_job_id() {
        let (mut scheduler, _rx) = Scheduler::new().await.expect("scheduler should initialize");
        let first = reconcile_schedules(
            &mut scheduler,
            &ScheduleSnapshot::default(),
            &[schedule("a", "*/5 * * * *")],
        )
        .await;

        let second =
            reconcile_schedules(&mut scheduler, &first, &[schedule("a", "*/10 * * * *")]).await;

        let first_id = first
            .registered
            .get("a")
            .expect("first snapshot should contain schedule a")
            .job_id;
        let second_id = second
            .registered
            .get("a")
            .expect("second snapshot should contain schedule a")
            .job_id;

        assert_ne!(first_id, second_id);
    }

    #[tokio::test]
    async fn test_reconcile_failed_remove_keeps_existing_snapshot_entry() {
        let (mut scheduler, _rx) = Scheduler::new().await.expect("scheduler should initialize");

        let mut current = ScheduleSnapshot::default();
        let fake_id = Uuid::new_v4();
        current.registered.insert(
            "zombie".to_string(),
            RegisteredSchedule {
                cron: "*/5 * * * *".to_string(),
                job_id: fake_id,
            },
        );

        let next = reconcile_schedules(&mut scheduler, &current, &[]).await;

        let retained = next
            .registered
            .get("zombie")
            .expect("failed removal should retain snapshot entry");
        assert_eq!(retained.job_id, fake_id);
    }

    #[tokio::test]
    async fn test_reconcile_ignores_disabled_schedules() {
        let (mut scheduler, _rx) = Scheduler::new().await.expect("scheduler should initialize");

        let next = reconcile_schedules(
            &mut scheduler,
            &ScheduleSnapshot::default(),
            &[disabled_schedule("off", "*/5 * * * *")],
        )
        .await;

        assert!(next.registered.is_empty());
    }

    #[tokio::test]
    async fn test_reconcile_remove_deleted_schedule() {
        let (mut scheduler, _rx) = Scheduler::new().await.expect("scheduler should initialize");
        let first = reconcile_schedules(
            &mut scheduler,
            &ScheduleSnapshot::default(),
            &[schedule("a", "*/5 * * * *"), schedule("b", "*/10 * * * *")],
        )
        .await;
        assert_eq!(first.registered.len(), 2);

        // Remove "b" from config.
        let second =
            reconcile_schedules(&mut scheduler, &first, &[schedule("a", "*/5 * * * *")]).await;

        assert_eq!(second.registered.len(), 1);
        assert!(second.registered.contains_key("a"));
        assert!(!second.registered.contains_key("b"));
        assert_eq!(scheduler.job_count(), 1);
    }

    #[tokio::test]
    async fn test_reconcile_reenable_after_disable() {
        let (mut scheduler, _rx) = Scheduler::new().await.expect("scheduler should initialize");

        // Start with "a" enabled.
        let first = reconcile_schedules(
            &mut scheduler,
            &ScheduleSnapshot::default(),
            &[schedule("a", "*/5 * * * *")],
        )
        .await;
        assert_eq!(first.registered.len(), 1);
        let original_id = first
            .registered
            .get("a")
            .expect("should have schedule a")
            .job_id;

        // Disable "a" — should be removed.
        let second = reconcile_schedules(
            &mut scheduler,
            &first,
            &[disabled_schedule("a", "*/5 * * * *")],
        )
        .await;
        assert!(second.registered.is_empty());
        assert_eq!(scheduler.job_count(), 0);

        // Re-enable "a" — should be re-added with new job id.
        let third =
            reconcile_schedules(&mut scheduler, &second, &[schedule("a", "*/5 * * * *")]).await;
        assert_eq!(third.registered.len(), 1);
        let reenabled_id = third
            .registered
            .get("a")
            .expect("should have schedule a after re-enable")
            .job_id;
        assert_ne!(original_id, reenabled_id);
        assert_eq!(scheduler.job_count(), 1);
    }

    #[tokio::test]
    async fn test_reconcile_reenable_with_changed_cron() {
        let (mut scheduler, _rx) = Scheduler::new().await.expect("scheduler should initialize");

        let first = reconcile_schedules(
            &mut scheduler,
            &ScheduleSnapshot::default(),
            &[schedule("a", "*/5 * * * *")],
        )
        .await;
        assert_eq!(first.registered.len(), 1);

        // Disable.
        let second = reconcile_schedules(
            &mut scheduler,
            &first,
            &[disabled_schedule("a", "*/5 * * * *")],
        )
        .await;
        assert!(second.registered.is_empty());

        // Re-enable with different cron.
        let third =
            reconcile_schedules(&mut scheduler, &second, &[schedule("a", "*/15 * * * *")]).await;
        assert_eq!(third.registered.len(), 1);
        assert_eq!(
            third
                .registered
                .get("a")
                .expect("should have schedule a")
                .cron,
            "*/15 * * * *"
        );
    }
}
