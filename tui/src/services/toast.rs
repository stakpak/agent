//! Toast notification system for the TUI.
//!
//! Provides brief, non-intrusive notifications that appear in the top-right
//! corner and automatically disappear after a short duration.

use std::time::{Duration, Instant};

/// A toast notification
#[derive(Debug, Clone)]
pub struct Toast {
    /// Message to display
    pub message: String,
    /// When the toast was created
    pub created_at: Instant,
    /// How long to display the toast
    pub duration: Duration,
    /// Visual style of the toast
    pub style: ToastStyle,
}

/// Visual style variants for toasts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastStyle {
    /// Success message (green background)
    Success,
    /// Error message (red background)
    Error,
    /// Informational message (blue background)
    Info,
}

impl Toast {
    /// Create a success toast
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            created_at: Instant::now(),
            duration: Duration::from_secs(2),
            style: ToastStyle::Success,
        }
    }

    /// Create an error toast
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            created_at: Instant::now(),
            duration: Duration::from_secs(3), // Errors stay longer
            style: ToastStyle::Error,
        }
    }

    /// Create an info toast
    #[allow(dead_code)]
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            created_at: Instant::now(),
            duration: Duration::from_secs(2),
            style: ToastStyle::Info,
        }
    }

    /// Check if the toast has expired
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.duration
    }
}
