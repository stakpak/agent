use stakpak_ak::StorageBackend;

use super::plan::SyncPlan;
use super::{Strategy, SyncDirection};

/// Per-file failure encountered during [`execute`].
///
/// Sync continues past failures (per the design doc's "network blip during
/// execute" edge case) and surfaces the full list in [`SyncReport::failures`]
/// so the caller can decide on the exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub path: String,
    pub error: String,
}

/// Outcome of [`execute`]. All vectors are sorted by path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub direction: SyncDirection,
    /// Paths successfully pushed (push only).
    pub uploaded: Vec<String>,
    /// Paths successfully pulled (pull only).
    pub downloaded: Vec<String>,
    /// Paths whose hashes already matched, plus any conflict skipped via
    /// `--strategy` ([`Strategy::Skip`] or the no-op side of the chosen
    /// strategy for that direction).
    pub skipped: Vec<String>,
    /// Conflict paths where the chosen `--strategy` performed an
    /// overwrite (e.g. `--strategy local` on a push pushed our local copy
    /// over the remote's).
    pub conflict_resolved: Vec<String>,
    /// Per-file errors. Sync did not abort; the file was simply skipped.
    pub failures: Vec<Failure>,
}

impl SyncReport {
    fn new(direction: SyncDirection) -> Self {
        Self {
            direction,
            uploaded: Vec::new(),
            downloaded: Vec::new(),
            skipped: Vec::new(),
            conflict_resolved: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Whether any file failed to sync. The caller maps this to exit code 1.
    pub fn has_failures(&self) -> bool {
        !self.failures.is_empty()
    }
}

/// Apply a [`SyncPlan`] against the two backends.
///
/// Execution is sequential and per-file fault-tolerant: a failure on one
/// path is recorded in [`SyncReport::failures`] but does not stop sync of
/// the remaining files
///
/// `strategy` is required if the plan contains conflicts. The CLI layer
/// (`run()`) is responsible for fail-fast handling — calling `execute()`
/// with conflicts but no strategy is a programming error and returns
/// `Err`.
///
///
/// | Strategy | Push (local → remote)              | Pull (remote → local)              |
/// |----------|------------------------------------|------------------------------------|
/// | Local    | overwrite remote with local        | keep local (skip download)         |
/// | Remote   | skip (don't push the local change) | overwrite local with remote        |
/// | Skip     | skip the conflict; sync the rest   | skip the conflict; sync the rest   |
pub fn execute(
    plan: SyncPlan,
    local: &dyn StorageBackend,
    remote: &dyn StorageBackend,
    strategy: Option<Strategy>,
) -> Result<SyncReport, String> {
    if !plan.conflicts.is_empty() && strategy.is_none() {
        return Err(format!(
            "execute() called with {} conflict(s) but no strategy",
            plan.conflicts.len()
        ));
    }

    if matches!(strategy, Some(Strategy::Recent)) {
        return Err(
            "--strategy recent is not yet supported. use local, remote, or skip".to_string(),
        );
    }

    let mut report = SyncReport::new(plan.direction);
    // Skipped paths from the plan (hash already matched) carry through
    // to the report so the user sees the full picture.
    report.skipped.extend(plan.skipped.iter().cloned());

    match plan.direction {
        SyncDirection::Push => execute_push(&plan, local, remote, strategy, &mut report),
        SyncDirection::Pull => execute_pull(&plan, local, remote, strategy, &mut report),
    }

    // Final ordering for deterministic output.
    report.uploaded.sort();
    report.downloaded.sort();
    report.skipped.sort();
    report.conflict_resolved.sort();
    report.failures.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(report)
}

fn execute_push(
    plan: &SyncPlan,
    local: &dyn StorageBackend,
    remote: &dyn StorageBackend,
    strategy: Option<Strategy>,
    report: &mut SyncReport,
) {
    // 1. Plain uploads (local-only files).
    for meta in &plan.uploads {
        match local.read(&meta.path) {
            Ok(body) => match remote.create(&meta.path, &body) {
                Ok(()) => report.uploaded.push(meta.path.clone()),
                Err(stakpak_ak::Error::AlreadyExists(_)) => {
                    // 409 race: file appeared on the remote between plan
                    // and execute. Don't silently overwrite
                    report.failures.push(Failure {
                        path: meta.path.clone(),
                        error: "remote file appeared between plan and execute (race). \
                                rerun `ak sync push` to re-evaluate"
                            .to_string(),
                    });
                }
                Err(e) => report.failures.push(Failure {
                    path: meta.path.clone(),
                    error: format!("upload failed: {e}"),
                }),
            },
            Err(e) => report.failures.push(Failure {
                path: meta.path.clone(),
                error: format!("local read failed: {e}"),
            }),
        }
    }

    // 2. Conflicts (apply strategy).
    for conflict in &plan.conflicts {
        match strategy {
            Some(Strategy::Local) => {
                // Overwrite remote with local.
                match local.read(&conflict.path) {
                    Ok(body) => match remote.overwrite(&conflict.path, &body) {
                        Ok(()) => report.conflict_resolved.push(conflict.path.clone()),
                        Err(e) => report.failures.push(Failure {
                            path: conflict.path.clone(),
                            error: format!("conflict overwrite (remote) failed: {e}"),
                        }),
                    },
                    Err(e) => report.failures.push(Failure {
                        path: conflict.path.clone(),
                        error: format!("local read for conflict failed: {e}"),
                    }),
                }
            }
            Some(Strategy::Remote) | Some(Strategy::Skip) => {
                report.skipped.push(conflict.path.clone());
            }
            Some(Strategy::Recent) | None => {
                // Already validated in execute(); unreachable here.
                report.failures.push(Failure {
                    path: conflict.path.clone(),
                    error: "internal: conflict reached execute_push without resolvable strategy"
                        .to_string(),
                });
            }
        }
    }
}

fn execute_pull(
    plan: &SyncPlan,
    local: &dyn StorageBackend,
    remote: &dyn StorageBackend,
    strategy: Option<Strategy>,
    report: &mut SyncReport,
) {
    // 1. Plain downloads (remote-only files).
    for meta in &plan.downloads {
        match remote.read(&meta.path) {
            Ok(body) => match local.create(&meta.path, &body) {
                Ok(()) => report.downloaded.push(meta.path.clone()),
                Err(stakpak_ak::Error::AlreadyExists(_)) => {
                    // Local race: file appeared locally between plan and
                    // execute (e.g. concurrent `ak write`). Same handling
                    // as the push race.
                    report.failures.push(Failure {
                        path: meta.path.clone(),
                        error: "local file appeared between plan and execute (race). \
                                rerun `ak sync pull` to re-evaluate"
                            .to_string(),
                    });
                }
                Err(e) => report.failures.push(Failure {
                    path: meta.path.clone(),
                    error: format!("local write failed: {e}"),
                }),
            },
            Err(e) => report.failures.push(Failure {
                path: meta.path.clone(),
                error: format!("remote read failed: {e}"),
            }),
        }
    }

    // 2. Conflicts (apply strategy).
    for conflict in &plan.conflicts {
        match strategy {
            Some(Strategy::Remote) => {
                // Overwrite local with remote.
                match remote.read(&conflict.path) {
                    Ok(body) => match local.overwrite(&conflict.path, &body) {
                        Ok(()) => report.conflict_resolved.push(conflict.path.clone()),
                        Err(e) => report.failures.push(Failure {
                            path: conflict.path.clone(),
                            error: format!("conflict overwrite (local) failed: {e}"),
                        }),
                    },
                    Err(e) => report.failures.push(Failure {
                        path: conflict.path.clone(),
                        error: format!("remote read for conflict failed: {e}"),
                    }),
                }
            }
            Some(Strategy::Local) | Some(Strategy::Skip) => {
                // Pull: don't touch the local copy.
                report.skipped.push(conflict.path.clone());
            }
            Some(Strategy::Recent) | None => {
                report.failures.push(Failure {
                    path: conflict.path.clone(),
                    error: "internal: conflict reached execute_pull without resolvable strategy"
                        .to_string(),
                });
            }
        }
    }
}
