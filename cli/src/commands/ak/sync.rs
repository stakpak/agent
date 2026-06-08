//! `stakpak ak sync` — reconcile the local ak knowledge store with the
//! remote Stakpak knowledge store.

use clap::{Subcommand, ValueEnum};

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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use clap::Parser;

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
}
