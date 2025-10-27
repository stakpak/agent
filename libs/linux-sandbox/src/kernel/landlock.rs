use crate::error::Result;

/// Check if Landlock is supported on this system
pub fn check_support() -> bool {
    // Check kernel version (Landlock requires 5.13+)
    if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        if let Some(version) = parse_kernel_version(&release) {
            return version >= (5, 13);
        }
    }
    false
}

fn parse_kernel_version(s: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() >= 2 {
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        Some((major, minor))
    } else {
        None
    }
}

pub struct LandlockRules {
    // TODO: Implement Landlock rules using liblandlock
    // For now, we'll just provide the structure
}

impl LandlockRules {
    pub fn new() -> Self {
        LandlockRules {}
    }
    
    pub fn apply(&self) -> Result<()> {
        // TODO: Implement actual Landlock rules
        // This would use liblandlock or direct syscalls via nix
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_check_support() {
        // Just test that it doesn't panic
        let _ = check_support();
    }
    
    #[test]
    fn test_parse_kernel_version() {
        assert_eq!(parse_kernel_version("5.13.0"), Some((5, 13)));
        assert_eq!(parse_kernel_version("6.1.0"), Some((6, 1)));
    }
}

