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
}
