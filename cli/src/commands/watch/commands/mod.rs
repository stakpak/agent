//! Watch CLI commands.
//!
//! This module contains all the CLI subcommands for the watch feature.

mod history;
mod init;
mod install;
mod prune;
mod reload;
mod resume;
mod run;
mod service;
mod status;
mod stop;
mod trigger;
mod uninstall;

use clap::Subcommand;

// Re-export command functions
pub use history::{show_history, show_run};
pub use init::init_config;
pub use install::install_watch;
pub use prune::prune_history;
pub use reload::reload_watch;
pub use resume::resume_run;
pub use run::run_watch;
pub use status::show_status;
pub use stop::stop_watch;
pub use trigger::{fire_trigger, show_trigger};
pub use uninstall::uninstall_watch;

/// Watch subcommands for managing the autonomous agent scheduler.
#[derive(Subcommand, PartialEq, Debug)]
pub enum WatchCommands {
    /// Start the watch service in foreground mode
    Run,
    /// Stop a running watch service
    Stop,
    /// Show watch status overview
    Status,
    /// List resources (triggers, runs)
    Get {
        #[command(subcommand)]
        resource: GetResource,
    },
    /// Show detailed information about a resource
    Describe {
        #[command(subcommand)]
        resource: DescribeResource,
    },
    /// Manually fire a trigger
    Fire {
        /// Name of the trigger to fire
        trigger: String,
        /// Show what would happen without actually running
        #[arg(long)]
        dry_run: bool,
    },
    /// Resume an interrupted agent run
    Resume {
        /// Run ID to resume
        run_id: i64,
        /// Resume even if run already completed
        #[arg(short, long)]
        force: bool,
    },
    /// Clean up old run history
    Prune {
        /// Delete runs older than this many days
        #[arg(short, long, default_value = "30")]
        days: u32,
    },
    /// Create a sample configuration file
    Init {
        /// Overwrite existing configuration
        #[arg(short, long)]
        force: bool,
    },
    /// Install watch as a system service (launchd on macOS, systemd on Linux)
    Install {
        /// Reinstall even if already installed
        #[arg(short, long)]
        force: bool,
    },
    /// Uninstall watch system service
    Uninstall,
    /// Reload watch configuration (restarts the service)
    Reload,
}

/// Resources for the 'get' command.
#[derive(Subcommand, PartialEq, Debug, Clone)]
pub enum GetResource {
    /// List configured triggers
    Triggers,
    /// List run history
    Runs {
        /// Filter by trigger name
        #[arg(short, long)]
        trigger: Option<String>,
        /// Maximum number of runs to show
        #[arg(short = 'n', long, default_value = "20")]
        limit: u32,
    },
}

/// Resources for the 'describe' command.
#[derive(Subcommand, PartialEq, Debug, Clone)]
pub enum DescribeResource {
    /// Show detailed information about a trigger
    Trigger {
        /// Name of the trigger
        name: String,
    },
    /// Show detailed information about a run
    Run {
        /// Run ID to inspect
        id: i64,
    },
}
