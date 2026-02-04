//! Local storage and hook infrastructure
//!
//! This module provides:
//! - Database operations for local session storage
//! - Lifecycle hooks for context management

// Sub-modules
pub(crate) mod context_managers;
pub mod hooks;
pub mod migrations;
pub mod storage;

#[cfg(test)]
mod tests;
