//! Autopilot module for autonomous agent scheduling.
//!
//! This module provides functionality for running the Stakpak agent as an autopilot service
//! with scheduled tasks, check scripts, and automatic agent invocation.

// Allow dead code in this module as it's still under development
// and some APIs are kept for future use or completeness
#![allow(dead_code)]

mod agent;
pub mod commands;
pub mod config;
mod db;
mod executor;
mod prompt;
mod reconciler;
mod scheduler;
mod utils;

pub use agent::{AgentServerConnection, SpawnConfig, spawn_agent};
pub use config::{DeliveryConfig, InteractionMode, Schedule, ScheduleConfig};
pub use db::{INTERACTIVE_DELEGATED_NOTE, ListRunsFilter, RELOAD_SENTINEL, RunStatus, ScheduleDb};
pub use executor::{CheckResult, run_check_script};
pub use prompt::{assemble_prompt, build_schedule_caller_context};
pub use scheduler::Scheduler;
pub use utils::is_process_running;
