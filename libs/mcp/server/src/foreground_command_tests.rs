// Fixture helpers intentionally fail fast on setup/transport errors.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use tokio::process::Command;

fn successful_child() -> ForegroundCommand {
    ForegroundCommand::new(
        Command::new("sh")
            .arg("-c")
            .arg("exit 7")
            .process_group(0)
            .spawn()
            .unwrap(),
    )
}

#[tokio::test]
async fn foreground_wait_disarms_group_after_reap() {
    let mut owned = successful_child();
    assert_eq!(owned.wait().await.unwrap().code(), Some(7));
    assert!(owned.child_mut().id().is_none());
    // No numeric authority remains even if Drop is delayed.
    owned.kill_group().unwrap();
    drop(owned);
}

#[tokio::test]
async fn foreground_try_wait_disarms_group_after_reap() {
    let mut owned = successful_child();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(status) = owned.child_mut().try_wait().unwrap() {
                assert_eq!(status.code(), Some(7));
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(owned.child_mut().id().is_none());
    owned.kill_group().unwrap();
    drop(owned);
}

#[tokio::test]
async fn foreground_external_reap_disarms_signal_authority() {
    let mut owned = successful_child();
    let pid = owned.child_mut().id().unwrap() as libc::pid_t;
    let mut status = 0;
    // SAFETY: reap only this test's exact owned child, simulating an external
    // reaper. It immediately exits; no global waitpid(-1) or PID reuse tricks.
    assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
    let error = owned.wait().await.unwrap_err();
    assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
    assert!(!owned.owned);
    owned.kill_group().unwrap();
    drop(owned);
}
