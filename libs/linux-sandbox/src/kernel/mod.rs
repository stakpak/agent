pub mod landlock;
pub mod seccomp;

pub use landlock::LandlockRules;
pub use seccomp::SeccompFilter;

// Check if Landlock is supported on this system
pub fn is_landlock_supported() -> bool {
    landlock::check_support()
}

// Check if seccomp is supported on this system
pub fn is_seccomp_supported() -> bool {
    seccomp::check_support()
}

// Check if running in container
pub fn is_running_in_container() -> bool {
    std::path::Path::new("/.dockerenv").exists() ||
    (std::path::Path::new("/proc/1/cgroup").exists() && 
     std::fs::read_to_string("/proc/1/cgroup")
         .map(|content| {
             content.contains("docker") || 
             content.contains("containerd") || 
             content.contains("kubepods")
         })
         .unwrap_or(false))
}

