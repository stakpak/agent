//! Watch module for autonomous agent scheduling.
//!
//! This module provides functionality for running the Stakpak agent as a watch service
//! with scheduled triggers, check scripts, and automatic agent invocation.

// Allow dead code in this module as it's still under development
// and some APIs are kept for future use or completeness
#![allow(dead_code)]

mod agent;
pub mod commands;
pub mod config;
mod db;
mod executor;
mod prompt;
mod scheduler;
mod utils;

pub use agent::{SpawnConfig, spawn_agent};
pub use commands::WatchCommands;
pub use config::{DeliveryConfig, Trigger, WatchConfig};
pub use db::{ListRunsFilter, RunStatus, WatchDb};
pub use executor::{CheckResult, run_check_script};
pub use prompt::assemble_prompt;
pub use scheduler::WatchScheduler;
pub use utils::is_process_running;
