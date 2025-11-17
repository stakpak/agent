//! Update Module
//!
//! This module re-exports the update function from the handlers module.
//! All event handling logic has been moved to handlers/ submodules for better organization.

pub use crate::services::handlers::update;
