use std::io;
use std::process::ExitStatus;
use tokio::process::Child;

/// Owns a foreground child's group until the child is reaped. Commands that
/// deliberately leave this group are outside this cancellation boundary.
pub(crate) struct ForegroundCommand {
    child: Child,
    owned: bool,
}

impl ForegroundCommand {
    pub(crate) fn new(child: Child) -> Self {
        Self { child, owned: true }
    }

    pub(crate) fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    #[cfg(unix)]
    fn kill_group(&mut self) -> io::Result<()> {
        if !self.owned {
            return Ok(());
        }
        if let Some(pid) = self.child.id() {
            let pid = libc::pid_t::try_from(pid)
                .map_err(|_| io::Error::other("invalid foreground process ID"))?;
            if pid <= 1 {
                return Err(io::Error::other("invalid foreground process group"));
            }
            // SAFETY: process_group(0) made this owned child the group leader.
            // Child::id() becomes None after wait/try_wait successfully reaps it;
            // while unreaped (even a zombie), its PID cannot be reused.
            if unsafe { libc::kill(-pid, libc::SIGKILL) } == -1 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn wait(&mut self) -> io::Result<ExitStatus> {
        let result = self.child.wait().await;
        match &result {
            Ok(_) => self.owned = false,
            // Child::id remains Some on a wait error. ECHILD means the
            // exclusive ownership assumption was broken by another reaper;
            // never signal that numeric PID/group again, including in Drop.
            #[cfg(unix)]
            Err(error) if error.raw_os_error() == Some(libc::ECHILD) => self.owned = false,
            Err(_) => {}
        }
        result
    }

    pub(crate) async fn terminate(&mut self) -> io::Result<ExitStatus> {
        #[cfg(unix)]
        self.kill_group()?;
        #[cfg(not(unix))]
        self.child.start_kill()?;
        tokio::time::timeout(std::time::Duration::from_secs(5), self.wait())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "foreground cleanup timed out"))?
    }
}

impl Drop for ForegroundCommand {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        #[cfg(unix)]
        if let Err(error) = self.kill_group() {
            tracing::error!("Failed to kill foreground command group: {error}");
        }
        // Direct-child fallback is also restricted to a still-owned child.
        // Command's default kill_on_drop(false) avoids a deferred stale-PID
        // kill after ECHILD; Tokio still handles reaping when Child is dropped.
        if self.child.id().is_some() {
            let _ = self.child.start_kill();
        }
    }
}

#[cfg(all(test, unix))]
#[path = "foreground_command_tests.rs"]
mod tests;
