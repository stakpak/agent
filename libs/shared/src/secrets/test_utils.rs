//! Test utilities for secret detection tests.
//!
//! This module provides shared test configurations to avoid recompiling
//! regex patterns for every test, which would be slow and memory-intensive.

use super::gitleaks::{GitleaksConfig, create_gitleaks_config};
use std::sync::LazyLock;

/// Lazy-loaded gitleaks configuration
pub static TEST_GITLEAKS_CONFIG: LazyLock<GitleaksConfig> =
    LazyLock::new(|| create_gitleaks_config(false));

/// Lazy-loaded gitleaks configuration with privacy rules
pub static TEST_GITLEAKS_CONFIG_WITH_PRIVACY: LazyLock<GitleaksConfig> =
    LazyLock::new(|| create_gitleaks_config(true));
