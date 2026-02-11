//! Check script executor with timeout support.
//!
//! Executes check scripts as child processes, capturing output and enforcing timeouts.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{debug, warn};

/// Result of running a check script.
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Exit code of the script (None if killed/timed out before exit).
    pub exit_code: Option<i32>,
    /// Standard output captured from the script.
    pub stdout: String,
    /// Standard error captured from the script.
    pub stderr: String,
    /// Whether the script was killed due to timeout.
    pub timed_out: bool,
}

impl CheckResult {
    /// Returns true if the check passed (exit code 0).
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0)
    }

    /// Returns true if the check was skipped (exit code 1).
    pub fn skipped(&self) -> bool {
        self.exit_code == Some(1)
    }

    /// Returns true if the check failed (exit code 2+ or timeout).
    pub fn failed(&self) -> bool {
        self.timed_out || matches!(self.exit_code, Some(code) if code >= 2)
    }
}

/// Errors that can occur during check script execution.
#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    #[error("Script not found: {0}")]
    ScriptNotFound(String),

    #[error("Script is not executable: {0}")]
    NotExecutable(String),

    #[error("Failed to spawn process: {0}")]
    SpawnError(String),

    #[error("Failed to read output: {0}")]
    OutputError(String),
}

/// Run a check script with timeout enforcement.
///
/// # Arguments
/// * `path` - Path to the script to execute
/// * `timeout` - Maximum time to wait for the script to complete
///
/// # Returns
/// * `Ok(CheckResult)` - Script completed (possibly with timeout)
/// * `Err(ExecutorError)` - Failed to run the script
pub async fn run_check_script(
    path: &Path,
    timeout: Duration,
) -> Result<CheckResult, ExecutorError> {
    // Verify script exists
    if !path.exists() {
        return Err(ExecutorError::ScriptNotFound(path.display().to_string()));
    }

    // On Unix, verify script is executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata =
            std::fs::metadata(path).map_err(|e| ExecutorError::ScriptNotFound(e.to_string()))?;
        let permissions = metadata.permissions();
        if permissions.mode() & 0o111 == 0 {
            return Err(ExecutorError::NotExecutable(path.display().to_string()));
        }
    }

    debug!(script = %path.display(), timeout_secs = timeout.as_secs(), "Running check script");

    // Spawn the process
    let mut child = Command::new(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| ExecutorError::SpawnError(e.to_string()))?;

    // Take ownership of stdout/stderr handles
    let mut stdout_handle = child.stdout.take();
    let mut stderr_handle = child.stderr.take();

    // Run with timeout
    let result = tokio::time::timeout(timeout, async {
        // Read stdout and stderr concurrently
        let stdout_future = async {
            let mut buf = Vec::new();
            if let Some(ref mut handle) = stdout_handle {
                handle.read_to_end(&mut buf).await.ok();
            }
            String::from_utf8_lossy(&buf).to_string()
        };

        let stderr_future = async {
            let mut buf = Vec::new();
            if let Some(ref mut handle) = stderr_handle {
                handle.read_to_end(&mut buf).await.ok();
            }
            String::from_utf8_lossy(&buf).to_string()
        };

        let (stdout, stderr) = tokio::join!(stdout_future, stderr_future);

        // Wait for the process to exit
        let status = child.wait().await;

        (stdout, stderr, status)
    })
    .await;

    match result {
        Ok((stdout, stderr, status)) => {
            let exit_code = status.ok().and_then(|s| s.code());

            debug!(
                script = %path.display(),
                exit_code = ?exit_code,
                stdout_len = stdout.len(),
                stderr_len = stderr.len(),
                "Check script completed"
            );

            Ok(CheckResult {
                exit_code,
                stdout,
                stderr,
                timed_out: false,
            })
        }
        Err(_) => {
            // Timeout occurred - process will be killed by kill_on_drop
            warn!(
                script = %path.display(),
                timeout_secs = timeout.as_secs(),
                "Check script timed out"
            );

            // Try to read any partial output
            let stdout = if let Some(mut handle) = stdout_handle {
                let mut buf = Vec::new();
                handle.read_to_end(&mut buf).await.ok();
                String::from_utf8_lossy(&buf).to_string()
            } else {
                String::new()
            };

            let stderr = if let Some(mut handle) = stderr_handle {
                let mut buf = Vec::new();
                handle.read_to_end(&mut buf).await.ok();
                String::from_utf8_lossy(&buf).to_string()
            } else {
                String::new()
            };

            Ok(CheckResult {
                exit_code: None,
                stdout,
                stderr,
                timed_out: true,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::fs::{self, File};
    #[cfg(unix)]
    use std::io::Write;
    #[cfg(unix)]
    use tempfile::tempdir;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    /// Create a test script that exits with the given code.
    #[cfg(unix)]
    fn create_script(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let script_path = dir.join(name);
        {
            let mut file = File::create(&script_path).expect("Failed to create script");
            file.write_all(content.as_bytes())
                .expect("Failed to write script");
            file.sync_all().expect("Failed to sync script");
            // file is dropped (closed) here before we try to execute it
        }

        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&script_path)
                .expect("Failed to get metadata")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms).expect("Failed to set permissions");
        }

        script_path
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_script_exit_0() {
        let dir = tempdir().expect("Failed to create temp dir");
        let script = create_script(
            dir.path(),
            "check1.sh",
            "#!/bin/sh\necho 'context data'\nexit 0\n",
        );

        let result = run_check_script(&script, Duration::from_secs(5))
            .await
            .expect("Should run script");

        assert_eq!(result.exit_code, Some(0));
        assert!(result.passed());
        assert!(!result.skipped());
        assert!(!result.failed());
        assert!(!result.timed_out);
        assert!(result.stdout.contains("context data"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_script_exit_1() {
        let dir = tempdir().expect("Failed to create temp dir");
        let script = create_script(
            dir.path(),
            "check2.sh",
            "#!/bin/sh\necho 'not ready'\nexit 1\n",
        );

        let result = run_check_script(&script, Duration::from_secs(5))
            .await
            .expect("Should run script");

        assert_eq!(result.exit_code, Some(1));
        assert!(!result.passed());
        assert!(result.skipped());
        assert!(!result.failed());
        assert!(!result.timed_out);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_script_exit_2() {
        let dir = tempdir().expect("Failed to create temp dir");
        let script = create_script(
            dir.path(),
            "check3.sh",
            "#!/bin/sh\necho 'error occurred' >&2\nexit 2\n",
        );

        let result = run_check_script(&script, Duration::from_secs(5))
            .await
            .expect("Should run script");

        assert_eq!(result.exit_code, Some(2));
        assert!(!result.passed());
        assert!(!result.skipped());
        assert!(result.failed());
        assert!(!result.timed_out);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_script_timeout() {
        let dir = tempdir().expect("Failed to create temp dir");
        let script = create_script(dir.path(), "check4.sh", "#!/bin/sh\nsleep 10\nexit 0\n");

        let result = run_check_script(&script, Duration::from_millis(100))
            .await
            .expect("Should run script");

        assert!(result.timed_out);
        assert!(result.failed());
        assert!(result.exit_code.is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_capture_stderr() {
        let dir = tempdir().expect("Failed to create temp dir");
        let script = create_script(
            dir.path(),
            "check5.sh",
            "#!/bin/sh\necho 'stdout message'\necho 'stderr message' >&2\nexit 0\n",
        );

        let result = run_check_script(&script, Duration::from_secs(5))
            .await
            .expect("Should run script");

        assert!(result.stdout.contains("stdout message"));
        assert!(result.stderr.contains("stderr message"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_missing_script() {
        let result =
            run_check_script(Path::new("/nonexistent/script.sh"), Duration::from_secs(5)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutorError::ScriptNotFound(_)
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_non_executable() {
        let dir = tempdir().expect("Failed to create temp dir");
        let script_path = dir.path().join("check6.sh");

        // Create file without execute permission
        let mut file = File::create(&script_path).expect("Failed to create script");
        file.write_all(b"#!/bin/sh\nexit 0\n")
            .expect("Failed to write script");

        let mut perms = fs::metadata(&script_path)
            .expect("Failed to get metadata")
            .permissions();
        perms.set_mode(0o644); // No execute permission
        fs::set_permissions(&script_path, perms).expect("Failed to set permissions");

        let result = run_check_script(&script_path, Duration::from_secs(5)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutorError::NotExecutable(_)
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_large_output() {
        let dir = tempdir().expect("Failed to create temp dir");
        // Generate a script that outputs a lot of data
        let script = create_script(
            dir.path(),
            "check7.sh",
            "#!/bin/sh\nfor i in $(seq 1 1000); do echo \"line $i\"; done\nexit 0\n",
        );

        let result = run_check_script(&script, Duration::from_secs(10))
            .await
            .expect("Should run script");

        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.lines().count() >= 1000);
    }

    #[tokio::test]
    async fn test_check_result_methods() {
        // Test passed
        let passed = CheckResult {
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
        };
        assert!(passed.passed());
        assert!(!passed.skipped());
        assert!(!passed.failed());

        // Test skipped
        let skipped = CheckResult {
            exit_code: Some(1),
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
        };
        assert!(!skipped.passed());
        assert!(skipped.skipped());
        assert!(!skipped.failed());

        // Test failed (exit code 2)
        let failed = CheckResult {
            exit_code: Some(2),
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
        };
        assert!(!failed.passed());
        assert!(!failed.skipped());
        assert!(failed.failed());

        // Test timed out
        let timed_out = CheckResult {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: true,
        };
        assert!(!timed_out.passed());
        assert!(!timed_out.skipped());
        assert!(timed_out.failed());
    }
}
