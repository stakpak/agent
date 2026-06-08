//! `stakpak ak sync` — reconcile the local ak knowledge store with the
//! remote Stakpak knowledge store.

use clap::{Subcommand, ValueEnum};
use stakpak_ak::{FileMeta, StorageBackend};
use std::collections::HashMap;

use crate::config::AppConfig;

/// Conflict-resolution strategy applied when local and remote disagree on
/// the same path. Without `--strategy`, sync is fail-fast: it lists the
/// conflicts and exits non-zero so the user can pick a strategy.
///
/// Effect depends on direction (push vs pull):
///
/// | Strategy | `push` (local → remote)            | `pull` (remote → local)            |
/// |----------|------------------------------------|------------------------------------|
/// | Local    | overwrite remote with local        | keep local (skip download)         |
/// | Remote   | skip (don't push the local change) | overwrite local with remote        |
/// | Skip     | leave conflicts; sync the rest     | leave conflicts; sync the rest     |
/// | Recent   | overwrite older with newer         | overwrite older with newer         |
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum Strategy {
    /// Treat the local copy as authoritative for any conflict.
    Local,
    /// Treat the remote copy as authoritative for any conflict.
    Remote,
    /// Treat the newer copy as authoritative for any conflict.
    Recent,
    /// Skip conflicting files; sync everything else.
    Skip,
}

/// Direction of a sync operation. Set by the `push`/`pull` subcommand;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    /// Local → remote. Uploads local-only files; never deletes on the remote.
    Push,
    /// Remote → local. Downloads remote-only files; never deletes locally.
    Pull,
}

#[derive(Subcommand, PartialEq, Debug)]
#[command(
    about = "Sync the local ak knowledge store with the remote Stakpak knowledge store",
    long_about = "Reconcile the local ak knowledge store (~/.stakpak/knowledge or $AK_STORE) \
with the remote Stakpak knowledge store.

Sync is directional and additive:
- `push` copies local-only files up to the remote
- `pull` copies remote-only files down to the local store
- neither subcommand deletes files on either side

When the same path exists on both sides with different content, sync is \
fail-fast by default: it lists the conflicts and exits with code 2 so you \
can rerun with an explicit `--strategy`.",
    after_help = "Examples:
  stakpak ak sync push
  stakpak ak sync pull --dry-run
  stakpak ak sync push --strategy local
  stakpak ak sync pull --strategy remote"
)]
pub enum SyncCommand {
    #[command(
        about = "Push local-only files up to the remote knowledge store",
        long_about = "Upload files that exist locally but are missing on the remote.

Files identical on both sides are skipped (compared by SHA-256 content hash). \
Files that exist remotely but not locally are left alone \
When the same path exists on both sides with different content, sync stops \
and lists the conflicts. Rerun with `--strategy` to resolve."
    )]
    Push {
        /// Conflict-resolution strategy. Without this flag, sync is fail-fast on conflicts.
        #[arg(long, value_enum)]
        strategy: Option<Strategy>,

        /// Print the plan (uploads, skips, conflicts) without modifying anything.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    #[command(
        about = "Pull remote-only files down to the local knowledge store",
        long_about = "Download files that exist remotely but are missing locally.

Files identical on both sides are skipped (compared by SHA-256 content hash). \
Files that exist locally but not remotely are left alone \
When the same path exists on both sides with different content, sync stops \
and lists the conflicts. Rerun with `--strategy` to resolve."
    )]
    Pull {
        /// Conflict-resolution strategy. Without this flag, sync is fail-fast on conflicts.
        #[arg(long, value_enum)]
        strategy: Option<Strategy>,

        /// Print the plan (downloads, skips, conflicts) without modifying anything.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

impl SyncCommand {
    /// Direction of this subcommand (push vs pull).
    pub fn direction(&self) -> SyncDirection {
        match self {
            Self::Push { .. } => SyncDirection::Push,
            Self::Pull { .. } => SyncDirection::Pull,
        }
    }

    /// Whether `--dry-run` was passed.
    pub fn dry_run(&self) -> bool {
        match self {
            Self::Push { dry_run, .. } | Self::Pull { dry_run, .. } => *dry_run,
        }
    }

    /// User-supplied conflict-resolution strategy, if any. `None` means fail-fast.
    pub fn strategy(&self) -> Option<Strategy> {
        match self {
            Self::Push { strategy, .. } | Self::Pull { strategy, .. } => *strategy,
        }
    }
}

pub fn run(cmd: SyncCommand, _config: AppConfig) -> Result<(), String> {
    let _ = (cmd.direction(), cmd.dry_run(), cmd.strategy());
    todo!();
}

// ============================================================================
// Plan
// ============================================================================

/// A single conflict: same path on both sides, different content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: String,
    pub local_hash: String,
    pub remote_hash: String,
    pub local_size: u64,
    pub remote_size: u64,
}

/// The full reconciliation plan produced by [`plan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub direction: SyncDirection,
    pub uploads: Vec<FileMeta>,
    pub downloads: Vec<FileMeta>,
    pub skipped: Vec<String>,
    pub conflicts: Vec<Conflict>,
}

impl SyncPlan {
    pub fn is_empty(&self) -> bool {
        self.uploads.is_empty() && self.downloads.is_empty() && self.conflicts.is_empty()
    }
}

/// Build a [`SyncPlan`] by enumerating both sides and classifying each path.
///
/// Both backends are walked from the store root (`""`) and indexed by
/// path. The classification rules follow the following rules
///
/// | Local | Remote | Hashes | Push          | Pull           |
/// |-------|--------|--------|---------------|----------------|
/// | yes   | no     | —      | upload        | (silently skip)|
/// | no    | yes    | —      | (silently skip)| download      |
/// | yes   | yes    | match  | skip          | skip           |
/// | yes   | yes    | differ | conflict      | conflict       |
///
/// Output vectors are sorted by path for deterministic, diff-friendly output.
pub fn plan(
    local: &dyn StorageBackend,
    remote: &dyn StorageBackend,
    direction: SyncDirection,
) -> Result<SyncPlan, String> {
    let local_metas = local
        .list_with_meta("")
        .map_err(|e| format!("failed to enumerate local store: {e}"))?;
    let remote_metas = remote
        .list_with_meta("")
        .map_err(|e| format!("failed to enumerate remote store: {e}"))?;

    let mut local_index: HashMap<String, FileMeta> = local_metas
        .into_iter()
        .map(|meta| (meta.path.clone(), meta))
        .collect();
    let remote_index: HashMap<String, FileMeta> = remote_metas
        .into_iter()
        .map(|meta| (meta.path.clone(), meta))
        .collect();

    let mut uploads: Vec<FileMeta> = Vec::new();
    let mut downloads: Vec<FileMeta> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut conflicts: Vec<Conflict> = Vec::new();

    for (path, remote_meta) in &remote_index {
        match local_index.remove(path) {
            Some(local_meta) => {
                if local_meta.content_hash == remote_meta.content_hash {
                    skipped.push(path.clone());
                } else {
                    conflicts.push(Conflict {
                        path: path.clone(),
                        local_hash: local_meta.content_hash,
                        remote_hash: remote_meta.content_hash.clone(),
                        local_size: local_meta.size_bytes,
                        remote_size: remote_meta.size_bytes,
                    });
                }
            }
            None => {
                if matches!(direction, SyncDirection::Pull) {
                    downloads.push(remote_meta.clone());
                }
            }
        }
    }

    for (_path, local_meta) in local_index.drain() {
        if matches!(direction, SyncDirection::Push) {
            uploads.push(local_meta);
        }
    }

    uploads.sort_by(|a, b| a.path.cmp(&b.path));
    downloads.sort_by(|a, b| a.path.cmp(&b.path));
    skipped.sort();
    conflicts.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(SyncPlan {
        direction,
        uploads,
        downloads,
        skipped,
        conflicts,
    })
}

// ============================================================================
// Execute
// ============================================================================

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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use clap::Parser;
    use stakpak_ak::{LocalFsBackend, StorageBackend};

    #[derive(Parser, Debug)]
    struct TestCli {
        #[command(subcommand)]
        cmd: SyncCommand,
    }

    #[test]
    fn clap_definition_is_valid() {
        // Catches misuse of clap attributes at compile + load time.
        TestCli::command().debug_assert();
    }

    #[test]
    fn push_defaults() {
        let cli = TestCli::try_parse_from(["test", "push"]).expect("parse");
        assert_eq!(cli.cmd.direction(), SyncDirection::Push);
        assert!(!cli.cmd.dry_run());
        assert_eq!(cli.cmd.strategy(), None);
    }

    #[test]
    fn pull_with_dry_run_and_strategy() {
        let cli = TestCli::try_parse_from(["test", "pull", "--dry-run", "--strategy", "remote"])
            .expect("parse");
        assert_eq!(cli.cmd.direction(), SyncDirection::Pull);
        assert!(cli.cmd.dry_run());
        assert_eq!(cli.cmd.strategy(), Some(Strategy::Remote));
    }

    #[test]
    fn strategy_accepts_all_three_values() {
        for value in ["local", "remote", "skip"] {
            TestCli::try_parse_from(["test", "push", "--strategy", value])
                .unwrap_or_else(|e| panic!("strategy={value} should parse, got: {e}"));
        }
    }

    #[test]
    fn unknown_strategy_rejected() {
        let result = TestCli::try_parse_from(["test", "push", "--strategy", "force"]);
        assert!(result.is_err(), "unknown strategy should fail to parse");
    }

    /// Build a (local, remote) backend pair backed by tempdirs.
    /// The tempdirs are returned so callers can keep them alive for the
    /// duration of the test.
    fn pair() -> (tempfile::TempDir, LocalFsBackend, LocalFsBackend) {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let local = LocalFsBackend::with_root(temp.path().join("local"));
        let remote = LocalFsBackend::with_root(temp.path().join("remote"));
        (temp, local, remote)
    }

    #[test]
    fn plan_empty_stores() {
        let (_temp, local, remote) = pair();
        let p = plan(&local, &remote, SyncDirection::Push).expect("plan");

        assert!(p.uploads.is_empty());
        assert!(p.downloads.is_empty());
        assert!(p.skipped.is_empty());
        assert!(p.conflicts.is_empty());
        assert!(p.is_empty());
    }

    #[test]
    fn plan_push_uploads_local_only_files() {
        let (_temp, local, remote) = pair();
        local.create("notes/a.md", b"alpha").expect("create local");
        local
            .create("services/b.md", b"beta")
            .expect("create local");

        let p = plan(&local, &remote, SyncDirection::Push).expect("plan");

        assert_eq!(p.uploads.len(), 2);
        assert_eq!(p.uploads[0].path, "notes/a.md");
        assert_eq!(p.uploads[1].path, "services/b.md");
        assert!(p.downloads.is_empty());
        assert!(p.skipped.is_empty());
        assert!(p.conflicts.is_empty());
    }

    #[test]
    fn plan_pull_downloads_remote_only_files() {
        let (_temp, local, remote) = pair();
        remote
            .create("notes/a.md", b"alpha")
            .expect("create remote");
        remote
            .create("services/b.md", b"beta")
            .expect("create remote");

        let p = plan(&local, &remote, SyncDirection::Pull).expect("plan");

        assert_eq!(p.downloads.len(), 2);
        assert_eq!(p.downloads[0].path, "notes/a.md");
        assert_eq!(p.downloads[1].path, "services/b.md");
        assert!(p.uploads.is_empty());
        assert!(p.skipped.is_empty());
        assert!(p.conflicts.is_empty());
    }

    #[test]
    fn plan_push_ignores_remote_only_files() {
        // Sync is additive: a remote-only file should NOT appear in any
        // bucket on push (no delete-on-remote, no skipped entry either).
        let (_temp, local, remote) = pair();
        remote
            .create("only-remote.md", b"x")
            .expect("create remote");

        let p = plan(&local, &remote, SyncDirection::Push).expect("plan");

        assert!(p.uploads.is_empty());
        assert!(p.downloads.is_empty());
        assert!(p.skipped.is_empty());
        assert!(p.conflicts.is_empty());
    }

    #[test]
    fn plan_pull_ignores_local_only_files() {
        let (_temp, local, remote) = pair();
        local.create("only-local.md", b"x").expect("create local");

        let p = plan(&local, &remote, SyncDirection::Pull).expect("plan");

        assert!(p.uploads.is_empty());
        assert!(p.downloads.is_empty());
        assert!(p.skipped.is_empty());
        assert!(p.conflicts.is_empty());
    }

    #[test]
    fn plan_skips_identical_files() {
        let (_temp, local, remote) = pair();
        local.create("shared.md", b"same").expect("create local");
        remote.create("shared.md", b"same").expect("create remote");

        let p = plan(&local, &remote, SyncDirection::Push).expect("plan");

        assert_eq!(p.skipped, vec!["shared.md".to_string()]);
        assert!(p.uploads.is_empty());
        assert!(p.conflicts.is_empty());
        assert!(p.is_empty());
    }

    #[test]
    fn plan_detects_conflicts() {
        let (_temp, local, remote) = pair();
        local
            .create("shared.md", b"local-version")
            .expect("create local");
        remote
            .create("shared.md", b"remote-version")
            .expect("create remote");

        let p = plan(&local, &remote, SyncDirection::Push).expect("plan");

        assert_eq!(p.conflicts.len(), 1);
        let c = &p.conflicts[0];
        assert_eq!(c.path, "shared.md");
        assert_ne!(c.local_hash, c.remote_hash);
        assert_eq!(c.local_size, b"local-version".len() as u64);
        assert_eq!(c.remote_size, b"remote-version".len() as u64);
        assert!(p.uploads.is_empty());
        assert!(p.skipped.is_empty());
        assert!(!p.is_empty(), "conflicts make the plan non-noop");
    }

    #[test]
    fn plan_combines_all_categories() {
        // Mixed scenario:
        //   only-local.md   -> upload (push)
        //   only-remote.md  -> dropped (additive)
        //   identical.md    -> skipped
        //   conflict.md     -> conflict
        let (_temp, local, remote) = pair();
        local
            .create("only-local.md", b"L")
            .expect("create only-local");
        remote
            .create("only-remote.md", b"R")
            .expect("create only-remote");
        local
            .create("identical.md", b"same")
            .expect("create identical local");
        remote
            .create("identical.md", b"same")
            .expect("create identical remote");
        local
            .create("conflict.md", b"local-side")
            .expect("create conflict local");
        remote
            .create("conflict.md", b"remote-side")
            .expect("create conflict remote");

        let p = plan(&local, &remote, SyncDirection::Push).expect("plan");

        assert_eq!(
            p.uploads
                .iter()
                .map(|m| m.path.as_str())
                .collect::<Vec<_>>(),
            vec!["only-local.md"]
        );
        assert_eq!(p.skipped, vec!["identical.md".to_string()]);
        assert_eq!(p.conflicts.len(), 1);
        assert_eq!(p.conflicts[0].path, "conflict.md");
        assert!(p.downloads.is_empty());
    }

    #[test]
    fn plan_outputs_are_sorted() {
        // Deterministic ordering matters for cache-friendly diff output
        // and for tests downstream.
        let (_temp, local, remote) = pair();
        // Create in a deliberately non-alphabetical order.
        for path in ["zebra.md", "alpha.md", "mango.md"] {
            local.create(path, b"x").expect("create local");
        }
        for path in ["yak.md", "apple.md"] {
            remote.create(path, b"x").expect("create remote");
        }

        let p = plan(&local, &remote, SyncDirection::Push).expect("plan");

        let upload_paths: Vec<&str> = p.uploads.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(upload_paths, vec!["alpha.md", "mango.md", "zebra.md"]);
    }

    // ------------------------------------------------------------------
    // execute() tests
    // ------------------------------------------------------------------

    /// Convenience: build a plan and immediately execute it. The two
    /// halves are tested independently above; here we just want the
    /// end-to-end behavior.
    fn plan_and_execute(
        local: &LocalFsBackend,
        remote: &LocalFsBackend,
        direction: SyncDirection,
        strategy: Option<Strategy>,
    ) -> SyncReport {
        let p = plan(local, remote, direction).expect("plan");
        execute(p, local, remote, strategy).expect("execute")
    }

    #[test]
    fn execute_empty_plan_is_noop() {
        let (_temp, local, remote) = pair();

        let report = plan_and_execute(&local, &remote, SyncDirection::Push, None);

        assert!(report.uploaded.is_empty());
        assert!(report.downloaded.is_empty());
        assert!(report.skipped.is_empty());
        assert!(report.conflict_resolved.is_empty());
        assert!(!report.has_failures());
    }

    #[test]
    fn execute_push_uploads_files() {
        let (_temp, local, remote) = pair();
        local.create("alpha.md", b"A").expect("create alpha");
        local.create("nested/beta.md", b"B").expect("create beta");

        let report = plan_and_execute(&local, &remote, SyncDirection::Push, None);

        assert_eq!(report.uploaded, vec!["alpha.md", "nested/beta.md"]);
        // Bodies actually landed on the "remote".
        assert_eq!(remote.read("alpha.md").expect("read alpha"), b"A");
        assert_eq!(remote.read("nested/beta.md").expect("read beta"), b"B");
        assert!(!report.has_failures());
    }

    #[test]
    fn execute_pull_downloads_files() {
        let (_temp, local, remote) = pair();
        remote.create("alpha.md", b"A").expect("create alpha");
        remote.create("nested/beta.md", b"B").expect("create beta");

        let report = plan_and_execute(&local, &remote, SyncDirection::Pull, None);

        assert_eq!(report.downloaded, vec!["alpha.md", "nested/beta.md"]);
        assert_eq!(local.read("alpha.md").expect("read alpha"), b"A");
        assert_eq!(local.read("nested/beta.md").expect("read beta"), b"B");
        assert!(!report.has_failures());
    }

    #[test]
    fn execute_carries_plan_skipped_into_report() {
        // Files identical on both sides should appear in the report's
        // `skipped` list so the user gets a complete picture.
        let (_temp, local, remote) = pair();
        local.create("same.md", b"x").expect("create local");
        remote.create("same.md", b"x").expect("create remote");

        let report = plan_and_execute(&local, &remote, SyncDirection::Push, None);

        assert_eq!(report.skipped, vec!["same.md"]);
        assert!(report.uploaded.is_empty());
    }

    #[test]
    fn execute_push_strategy_local_overwrites_remote_on_conflict() {
        let (_temp, local, remote) = pair();
        local.create("c.md", b"local-wins").expect("create local");
        remote.create("c.md", b"old-remote").expect("create remote");

        let report = plan_and_execute(&local, &remote, SyncDirection::Push, Some(Strategy::Local));

        assert_eq!(report.conflict_resolved, vec!["c.md"]);
        assert_eq!(remote.read("c.md").expect("read remote"), b"local-wins");
        assert!(report.skipped.is_empty());
        assert!(!report.has_failures());
    }

    #[test]
    fn execute_push_strategy_remote_skips_conflict() {
        let (_temp, local, remote) = pair();
        local
            .create("c.md", b"local-version")
            .expect("create local");
        remote
            .create("c.md", b"remote-version")
            .expect("create remote");

        let report = plan_and_execute(&local, &remote, SyncDirection::Push, Some(Strategy::Remote));

        assert_eq!(report.skipped, vec!["c.md"]);
        assert!(report.conflict_resolved.is_empty());
        // Remote untouched.
        assert_eq!(remote.read("c.md").expect("read remote"), b"remote-version");
    }

    #[test]
    fn execute_push_strategy_skip_skips_conflict() {
        let (_temp, local, remote) = pair();
        local.create("c.md", b"L").expect("create local");
        remote.create("c.md", b"R").expect("create remote");

        let report = plan_and_execute(&local, &remote, SyncDirection::Push, Some(Strategy::Skip));

        assert_eq!(report.skipped, vec!["c.md"]);
        assert!(report.conflict_resolved.is_empty());
    }

    #[test]
    fn execute_pull_strategy_remote_overwrites_local_on_conflict() {
        let (_temp, local, remote) = pair();
        local.create("c.md", b"old-local").expect("create local");
        remote
            .create("c.md", b"remote-wins")
            .expect("create remote");

        let report = plan_and_execute(&local, &remote, SyncDirection::Pull, Some(Strategy::Remote));

        assert_eq!(report.conflict_resolved, vec!["c.md"]);
        assert_eq!(local.read("c.md").expect("read local"), b"remote-wins");
    }

    #[test]
    fn execute_pull_strategy_local_keeps_local_on_conflict() {
        let (_temp, local, remote) = pair();
        local.create("c.md", b"local-keeps").expect("create local");
        remote
            .create("c.md", b"remote-version")
            .expect("create remote");

        let report = plan_and_execute(&local, &remote, SyncDirection::Pull, Some(Strategy::Local));

        assert_eq!(report.skipped, vec!["c.md"]);
        assert!(report.conflict_resolved.is_empty());
        assert_eq!(local.read("c.md").expect("read local"), b"local-keeps");
    }

    #[test]
    fn execute_conflicts_without_strategy_errors() {
        // Defensive check: the CLI is supposed to fail-fast on conflicts
        // without a strategy. If a programmer reaches execute() with
        // conflicts and no strategy, they get a clear error rather than
        // silent corruption.
        let (_temp, local, remote) = pair();
        local.create("c.md", b"L").expect("create local");
        remote.create("c.md", b"R").expect("create remote");

        let p = plan(&local, &remote, SyncDirection::Push).expect("plan");
        assert_eq!(p.conflicts.len(), 1);

        let err = execute(p, &local, &remote, None).expect_err("must error");
        assert!(err.contains("conflict"), "got: {err}");
    }

    #[test]
    fn execute_strategy_recent_not_yet_supported() {
        // `Recent` is parseable but rejected at execute time until
        // FileMeta exposes timestamps. This test pins that contract.
        let (_temp, local, remote) = pair();
        // Need conflicts in the plan so strategy is even consulted.
        local.create("c.md", b"L").expect("create local");
        remote.create("c.md", b"R").expect("create remote");

        let p = plan(&local, &remote, SyncDirection::Push).expect("plan");
        let err =
            execute(p, &local, &remote, Some(Strategy::Recent)).expect_err("recent unsupported");
        assert!(err.contains("recent"), "got: {err}");
    }

    #[test]
    fn execute_failures_dont_abort_subsequent_files() {
        // Pre-create one of the upload targets on the "remote" side AFTER
        // planning. plan() sees the file as local-only (race-free in the
        // plan), then execute() hits a 409 race on `create`. Sync must
        // continue with the other file rather than abort.
        let (_temp, local, remote) = pair();
        local.create("good.md", b"g").expect("create good");
        local.create("racy.md", b"r-local").expect("create racy");

        let p = plan(&local, &remote, SyncDirection::Push).expect("plan");
        assert_eq!(p.uploads.len(), 2);

        // Inject the race: a different process created the file remotely
        // between plan and execute.
        remote
            .create("racy.md", b"r-remote")
            .expect("simulate race on remote");

        let report = execute(p, &local, &remote, None).expect("execute");

        // The good file made it through.
        assert_eq!(report.uploaded, vec!["good.md"]);
        // The racy file is recorded as a failure, not silently dropped.
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].path, "racy.md");
        assert!(
            report.failures[0].error.contains("race"),
            "expected race-aware message, got: {}",
            report.failures[0].error
        );
        // Remote still has the unrelated body — we did NOT overwrite it.
        assert_eq!(remote.read("racy.md").expect("read"), b"r-remote");
    }
}
