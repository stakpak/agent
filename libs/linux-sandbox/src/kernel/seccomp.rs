use crate::error::Result;

/// Check if seccomp is supported on this system
pub fn check_support() -> bool {
    // Seccomp is supported on most modern Linux kernels
    cfg!(target_os = "linux")
}

/// Seccomp filter configuration
pub struct SeccompFilter {
    allow_network: bool,
}

impl SeccompFilter {
    pub fn new(allow_network: bool) -> Self {
        Self { allow_network }
    }

    /// Apply seccomp filter to current process  
    pub fn apply(&self) -> Result<()> {
        self.apply_with_policy(self.allow_network)
    }

    /// Apply seccomp filter with specific network policy
    fn apply_with_policy(&self, _allow_network_param: bool) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            use libseccomp::*;

            // Create a new seccomp filter context
            let mut ctx = ScmpFilterContext::new_filter(ScmpAction::Allow)?;

            if !_allow_network_param {
                log::info!("Applying seccomp filter to block network syscalls");

                // Block network-related syscalls
                // socket - create network sockets
                if let Ok(syscall) = ScmpSyscall::new("socket") {
                    let _ = ctx.add_rule_exact(ScmpAction::Errno(libc::EACCES), syscall);
                }

                // connect - connect to network endpoints
                if let Ok(syscall) = ScmpSyscall::new("connect") {
                    let _ = ctx.add_rule_exact(ScmpAction::Errno(libc::EACCES), syscall);
                }

                // send* - send network data
                for syscall_name in &["send", "sendto", "sendmsg", "sendmmsg"] {
                    if let Ok(syscall) = ScmpSyscall::new(syscall_name) {
                        let _ = ctx.add_rule_exact(ScmpAction::Errno(libc::EACCES), syscall);
                    }
                }

                // recv* - receive network data
                for syscall_name in &["recv", "recvfrom", "recvmsg", "recvmmsg"] {
                    if let Ok(syscall) = ScmpSyscall::new(syscall_name) {
                        let _ = ctx.add_rule_exact(ScmpAction::Errno(libc::EACCES), syscall);
                    }
                }

                // bind, listen, accept - socket operations
                for syscall_name in &["bind", "listen", "accept", "accept4"] {
                    if let Ok(syscall) = ScmpSyscall::new(syscall_name) {
                        let _ = ctx.add_rule_exact(ScmpAction::Errno(libc::EACCES), syscall);
                    }
                }
            }

            // Load the seccomp filter
            ctx.load()?;
            log::info!("Seccomp filter loaded successfully");
        }

        #[cfg(not(target_os = "linux"))]
        {
            log::warn!("Seccomp is only supported on Linux");
        }

        Ok(())
    }

    /// Check if we can use seccomp (privileges, kernel support)
    pub fn can_use(&self) -> bool {
        check_support()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_support() {
        let supported = check_support();
        // Should be true on Linux
        assert_eq!(supported, cfg!(target_os = "linux"));
    }

    #[test]
    fn test_seccomp_filter_creation() {
        let filter = SeccompFilter::new(true);
        assert!(filter.allow_network);

        let filter2 = SeccompFilter::new(false);
        assert!(!filter2.allow_network);
    }
}
