use thiserror::Error;

#[derive(Error, Debug)]
pub enum SandboxError {
    #[error("Linux kernel version too old - Landlock requires kernel 5.13+")]
    KernelTooOld,

    #[error("Landlock not supported on this system")]
    LandlockNotSupported,

    #[error("Seccomp not supported on this system")]
    SeccompNotSupported,

    #[error("Policy parse error: {0}")]
    PolicyParseError(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Failed to set up network rules: {0}")]
    NetworkSetupFailed(String),

    #[error("Failed to fork process: {0}")]
    ForkFailed(String),

    #[error("Failed to apply kernel restrictions: {0}")]
    KernelRestrictionFailed(String),

    #[error("Audit logging failed: {0}")]
    AuditLoggingFailed(String),

    #[error("Policy validation failed: {0}")]
    PolicyValidationFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, SandboxError>;

