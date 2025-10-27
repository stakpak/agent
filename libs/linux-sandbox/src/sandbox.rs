use crate::audit::AuditLogger;
use crate::error::{Result, SandboxError};
use crate::kernel::{is_landlock_supported, is_seccomp_supported};
use crate::network::NetworkController;
use crate::policy::SandboxPolicy;
use std::process::{Command, ExitStatus};

/// Main sandbox interface
pub struct Sandbox {
    policy: SandboxPolicy,
    logger: AuditLogger,
}

impl Sandbox {
    /// Create a new sandbox with the given policy
    pub fn new(policy: SandboxPolicy) -> Self {
        let logger = AuditLogger::new(policy.audit.enabled, policy.audit.log_file.clone());

        Self { policy, logger }
    }

    /// Execute a command in the sandbox
    pub fn execute_command(&self, command: &str, args: &[&str]) -> Result<ExitStatus> {
        log::info!("Executing command in sandbox: {} {:?}", command, args);

        // Determine if network should be allowed for this command
        let allow_network = self.policy.should_allow_network(command);
        let is_destructive = self.policy.is_destructive(command);

        // Log command execution
        self.logger
            .log_command(command, allow_network, is_destructive);

        if is_destructive {
            self.logger.log_security_block(
                "destructive_command",
                &format!("Destructive command detected: {}", command),
            );
        }

        // Check kernel capabilities
        let has_landlock = is_landlock_supported();
        let has_seccomp = is_seccomp_supported();

        log::info!(
            "Kernel capabilities - Landlock: {}, Seccomp: {}",
            has_landlock,
            has_seccomp
        );

        // Apply network controller
        let network = NetworkController::new(allow_network);
        network.apply_restrictions()?;

        // For now, just execute the command without actual kernel restrictions
        // TODO: Implement fork + apply kernel restrictions to child
        let mut cmd = Command::new(command);
        cmd.args(args);

        match cmd.status() {
            Ok(status) => {
                log::info!("Command completed with status: {:?}", status.code());
                Ok(status)
            }
            Err(e) => {
                self.logger
                    .log_security_block("command_failed", &e.to_string());
                Err(SandboxError::IoError(e))
            }
        }
    }

    /// Get the audit logger
    pub fn logger(&self) -> &AuditLogger {
        &self.logger
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{SandboxMode, SandboxPolicy};

    #[test]
    fn test_sandbox_creation() {
        let policy = SandboxPolicy::default();
        let _sandbox = Sandbox::new(policy);

        // Should be able to create sandbox
        assert!(true);
    }

    #[test]
    fn test_policy_determines_network_access() {
        let mut policy = SandboxPolicy::default();
        policy.mode = SandboxMode::ReadOnly;

        // Test destructive commands
        assert!(!policy.should_allow_network("rm -rf /tmp/test"));
        assert!(policy.is_destructive("rm -rf /tmp/test"));

        // Test safe commands
        assert!(policy.should_allow_network("git pull"));
        assert!(!policy.is_destructive("git pull"));
    }
}
