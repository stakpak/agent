//! Autopilot CLI commands.
//!
//! This module contains all the CLI subcommands for the autopilot feature.

pub mod history;
mod init;
mod prune;
mod reload;
mod resume;
mod run;
pub mod schedule;
mod status;
mod stop;

use clap::Subcommand;

pub use run::run_scheduler;

/// Autopilot subcommands for managing the autonomous agent scheduler.
#[derive(Subcommand, PartialEq, Debug)]
pub enum ScheduleCommands {
    /// Start the autopilot service in foreground mode
    Run,
    /// Stop a running autopilot service
    Stop,
    /// Show autopilot status overview
    Status,
    /// List resources (schedules, runs)
    Get {
        #[command(subcommand)]
        resource: GetResource,
    },
    /// Show detailed information about a resource
    Describe {
        #[command(subcommand)]
        resource: DescribeResource,
    },
    /// Manually fire a schedule
    Fire {
        /// Name of the schedule to fire
        schedule: String,
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
    /// Reload autopilot configuration (restarts the service)
    Reload,
}

/// Resources for the 'get' command.
#[derive(Subcommand, PartialEq, Debug, Clone)]
pub enum GetResource {
    /// List configured schedules
    Schedules,
    /// List run history
    Runs {
        /// Filter by schedule name
        #[arg(short, long)]
        schedule: Option<String>,
        /// Maximum number of runs to show
        #[arg(short = 'n', long, default_value = "20")]
        limit: u32,
    },
}

/// Resources for the 'describe' command.
#[derive(Subcommand, PartialEq, Debug, Clone)]
pub enum DescribeResource {
    /// Show detailed information about a schedule
    Schedule {
        /// Name of the schedule
        name: String,
    },
    /// Show detailed information about a run
    Run {
        /// Run ID to inspect
        id: i64,
    },
}
