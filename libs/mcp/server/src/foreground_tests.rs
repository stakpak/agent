// Fixture helpers intentionally fail fast on setup/transport errors.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::tests::local_container_with_profile;
use super::*;
use rmcp::service::PeerRequestOptions;
use rmcp::{ServiceExt, model::ClientRequest};

struct ExecutionHarness {
    container: ToolContainer,
    abort_execution: bool,
}

impl rmcp::ServerHandler for ExecutionHarness {
    fn get_info(&self) -> ServerInfo {
        rmcp::ServerHandler::get_info(&self.container)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let command = request
            .arguments
            .as_ref()
            .and_then(|args| args.get("command"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if !self.abort_execution || command == "printf subsequent" {
            return rmcp::ServerHandler::call_tool(&self.container, request, ctx).await;
        }
        let container = self.container.clone();
        let ct = ctx.ct.clone();
        // This case aborts the execution future itself, independently of the
        // executor's ordinary request-token cancellation branches.
        let mut ctx = ctx;
        ctx.ct = Default::default();
        let execution =
            tokio::spawn(
                async move { container.execute_local_command(&command, None, &ctx).await },
            );
        ct.cancelled().await;
        execution.abort();
        assert!(execution.await.unwrap_err().is_cancelled());
        Ok(CallToolResult::cancel(None))
    }
}

fn process_identity(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    fields.split_whitespace().nth(19).map(str::to_owned)
}

async fn wait_for_marker(path: &Path) -> u32 {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = fs::read_to_string(path)
                && let Ok(pid) = text.trim().parse()
            {
                return pid;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("nested shell must author its start marker")
}

async fn assert_gone(pid: u32, identity: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let stat = fs::read_to_string(format!("/proc/{pid}/stat"));
            // A killed orphan may briefly remain a zombie under CI's
            // init; it cannot execute or create the delayed artifact.
            if process_identity(pid).as_deref() != Some(identity)
                || stat.as_ref().map_or(true, |s| {
                    s.rsplit_once(") ")
                        .is_some_and(|(_, rest)| rest.starts_with('Z'))
                })
            {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned shell must stop executing");
}

async fn prove_cleanup(
    owned: &[(u32, String)],
    late: &Path,
    control: &mut tokio::process::Child,
    identity: &str,
) {
    for (pid, identity) in owned {
        assert_gone(*pid, identity).await;
    }
    let control_pid = control.id().unwrap();
    assert!(
        control.try_wait().unwrap().is_none(),
        "unrelated process survives"
    );
    assert_eq!(process_identity(control_pid).as_deref(), Some(identity));
    sleep(Duration::from_millis(2200)).await;
    assert!(!late.exists(), "cancelled descendant must not write later");
    assert!(
        control.try_wait().unwrap().is_none(),
        "control survives deadline"
    );
    assert_eq!(process_identity(control_pid).as_deref(), Some(identity));
}

async fn exercise(kind: &str, configured_timeout: Option<u64>) {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("started");
    let late = dir.path().join("late");
    let outer = dir.path().join("outer");
    let sleeper = dir.path().join("sleeper");
    let completion = if kind == "leader-exit" {
        "exit 0"
    } else {
        "wait"
    };
    let command = format!(
        r#"echo $$ > "{}"; sh -c 'echo $$ > "{}"; sleep 2 & echo $! > "{}"; wait; echo late > "{}"' & {completion}"#,
        outer.display(),
        marker.display(),
        sleeper.display(),
        late.display()
    );
    let mut control = Command::new("sleep")
        .arg("30")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let control_pid = control.id().unwrap();
    let control_identity = process_identity(control_pid).unwrap();
    let (client_io, server_io) = tokio::io::duplex(65536);
    let abort_execution = kind == "abort";
    let server = tokio::spawn(async move {
        ExecutionHarness {
            container: local_container_with_profile(None),
            abort_execution,
        }
        .serve(server_io)
        .await
        .unwrap()
    });
    let client = ().serve(client_io).await.unwrap();
    let server = server.await.unwrap();
    let request = client
        .send_cancellable_request(
            ClientRequest::CallToolRequest(Request::new(CallToolRequestParam {
                name: "run_command".into(),
                arguments: Some(
                    serde_json::json!({
                        "command": command,
                        "timeout": configured_timeout,
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            })),
            PeerRequestOptions::default(),
        )
        .await
        .unwrap();
    let pid = wait_for_marker(&marker).await;
    let outer_pid = wait_for_marker(&outer).await;
    let sleeper_pid = wait_for_marker(&sleeper).await;
    let owned = [outer_pid, pid, sleeper_pid].map(|pid| (pid, process_identity(pid).unwrap()));
    if kind == "disconnect" {
        client.cancel().await.unwrap();
        server.cancel().await.unwrap();
        prove_cleanup(&owned, &late, &mut control, &control_identity).await;
    } else {
        if kind == "cancel" || kind == "abort" || kind == "leader-exit" {
            client
                .notify_cancelled(CancelledNotificationParam {
                    request_id: request.id.clone(),
                    reason: Some("regression cancellation".into()),
                })
                .await
                .unwrap();
        }
        let result = tokio::time::timeout(Duration::from_secs(5), request.await_response())
            .await
            .expect("executor must settle boundedly");
        if kind == "timeout" {
            let result = result.unwrap();
            assert!(format!("{result:?}").contains("timed out"));
        }
        prove_cleanup(&owned, &late, &mut control, &control_identity).await;
        // The same MCP session must still execute a subsequent command.
        let next = client
            .call_tool(CallToolRequestParam {
                name: "run_command".into(),
                arguments: Some(
                    serde_json::json!({"command": "printf subsequent"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            })
            .await
            .unwrap();
        assert!(format!("{next:?}").contains("subsequent"), "{next:?}");
        client.cancel().await.unwrap();
        server.cancel().await.unwrap();
    }
    control.kill().await.unwrap();
}

#[tokio::test]
async fn foreground_cancel_without_timeout() {
    exercise("cancel", None).await;
}

#[tokio::test]
async fn foreground_cancel_with_timeout() {
    exercise("cancel", Some(30)).await;
}

#[tokio::test]
async fn foreground_timeout() {
    exercise("timeout", Some(1)).await;
}

#[tokio::test]
async fn foreground_disconnect() {
    exercise("disconnect", None).await;
}

#[tokio::test]
async fn foreground_execution_future_abort() {
    exercise("abort", None).await;
}

#[tokio::test]
async fn foreground_cancel_after_leader_exit_before_pipe_eof() {
    exercise("leader-exit", None).await;
}
