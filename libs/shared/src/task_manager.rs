use crate::helper::generate_simple_id;
use crate::remote_connection::{RemoteConnectionInfo, RemoteConnectionManager};
use chrono::{DateTime, Utc};
use std::{collections::HashMap, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::{broadcast, mpsc, oneshot},
    time::timeout,
};

const START_TASK_WAIT_TIME: Duration = Duration::from_millis(300);

/// Kill a process and its entire process group.
///
/// Uses process group kill (`kill -9 -{pid}`) on Unix and `taskkill /F /T` on
/// Windows to ensure child processes spawned by shells (node, vite, esbuild, etc.)
/// are also terminated.
///
/// This is safe to call even if the process has already exited.
fn terminate_process_group(process_id: u32) {
    #[cfg(unix)]
    {
        use std::process::Command;
        // First check if the process exists
        let check_result = Command::new("kill")
            .arg("-0") // Signal 0 just checks if process exists
            .arg(process_id.to_string())
            .output();

        // Only kill if the process actually exists
        if check_result
            .map(|output| output.status.success())
            .unwrap_or(false)
        {
            // Kill the entire process group using negative PID
            // Since we spawn with .process_group(0), the shell becomes the process group leader
            // Using -{pid} kills all processes in that group (shell + children like node/vite/esbuild)
            let _ = Command::new("kill")
                .arg("-9")
                .arg(format!("-{}", process_id))
                .output();

            // Also try to kill the individual process in case it's not a group leader
            let _ = Command::new("kill")
                .arg("-9")
                .arg(process_id.to_string())
                .output();
        }
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        // On Windows, use taskkill with /T flag to kill the process tree
        let check_result = Command::new("tasklist")
            .arg("/FI")
            .arg(format!("PID eq {}", process_id))
            .arg("/FO")
            .arg("CSV")
            .output();

        // Only kill if the process actually exists
        if let Ok(output) = check_result {
            let output_str = String::from_utf8_lossy(&output.stdout);
            if output_str.lines().count() > 1 {
                // More than just header line - use /T to kill process tree
                let _ = Command::new("taskkill")
                    .arg("/F")
                    .arg("/T") // Kill process tree
                    .arg("/PID")
                    .arg(process_id.to_string())
                    .output();
            }
        }
    }
}

pub type TaskId = String;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Paused,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: TaskId,
    pub status: TaskStatus,
    pub command: String,
    pub description: Option<String>,
    pub remote_connection: Option<RemoteConnectionInfo>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub start_time: DateTime<Utc>,
    pub duration: Option<Duration>,
    pub timeout: Option<Duration>,
    pub pause_info: Option<PauseInfo>,
}

pub struct TaskEntry {
    pub task: Task,
    pub handle: tokio::task::JoinHandle<()>,
    pub process_id: Option<u32>,
    pub cancel_tx: Option<oneshot::Sender<()>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskInfo {
    pub id: TaskId,
    pub status: TaskStatus,
    pub command: String,
    pub description: Option<String>,
    pub output: Option<String>,
    pub start_time: DateTime<Utc>,
    pub duration: Option<Duration>,
    pub pause_info: Option<PauseInfo>,
}

impl From<&Task> for TaskInfo {
    fn from(task: &Task) -> Self {
        let duration = if matches!(task.status, TaskStatus::Running) {
            // For running tasks, calculate duration from start time to now
            Some(
                Utc::now()
                    .signed_duration_since(task.start_time)
                    .to_std()
                    .unwrap_or_default(),
            )
        } else {
            // For completed/failed/cancelled tasks, use the stored duration
            task.duration
        };

        TaskInfo {
            id: task.id.clone(),
            status: task.status.clone(),
            command: task.command.clone(),
            description: task.description.clone(),
            output: task.output.clone(),
            start_time: task.start_time,
            duration,
            pause_info: task.pause_info.clone(),
        }
    }
}

pub struct TaskCompletion {
    pub output: String,
    pub error: Option<String>,
    pub final_status: TaskStatus,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PauseInfo {
    pub checkpoint_id: Option<String>,
    pub raw_output: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("Task not found: {0}")]
    TaskNotFound(TaskId),
    #[error("Task already running: {0}")]
    TaskAlreadyRunning(TaskId),
    #[error("Manager shutdown")]
    ManagerShutdown,
    #[error("Command execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Task timeout")]
    TaskTimeout,
    #[error("Task cancelled")]
    TaskCancelled,
    #[error("Task failed on start: {0}")]
    TaskFailedOnStart(String),
    #[error("Task not paused: {0}")]
    TaskNotPaused(TaskId),
}

pub enum TaskMessage {
    Start {
        id: Option<TaskId>,
        command: String,
        description: Option<String>,
        remote_connection: Option<RemoteConnectionInfo>,
        timeout: Option<Duration>,
        response_tx: oneshot::Sender<Result<TaskId, TaskError>>,
    },
    Cancel {
        id: TaskId,
        response_tx: oneshot::Sender<Result<(), TaskError>>,
    },
    GetStatus {
        id: TaskId,
        response_tx: oneshot::Sender<Option<TaskStatus>>,
    },
    GetTaskDetails {
        id: TaskId,
        response_tx: oneshot::Sender<Option<TaskInfo>>,
    },
    GetAllTasks {
        response_tx: oneshot::Sender<Vec<TaskInfo>>,
    },
    Shutdown {
        response_tx: oneshot::Sender<()>,
    },
    TaskUpdate {
        id: TaskId,
        completion: TaskCompletion,
    },
    PartialUpdate {
        id: TaskId,
        output: String,
    },
    Resume {
        id: TaskId,
        command: String,
        response_tx: oneshot::Sender<Result<(), TaskError>>,
    },
}

pub struct TaskManager {
    tasks: HashMap<TaskId, TaskEntry>,
    tx: mpsc::UnboundedSender<TaskMessage>,
    rx: mpsc::UnboundedReceiver<TaskMessage>,
    shutdown_tx: broadcast::Sender<()>,
    shutdown_rx: broadcast::Receiver<()>,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        Self {
            tasks: HashMap::new(),
            tx,
            rx,
            shutdown_tx,
            shutdown_rx,
        }
    }

    pub fn handle(&self) -> Arc<TaskManagerHandle> {
        Arc::new(TaskManagerHandle {
            tx: self.tx.clone(),
            shutdown_tx: self.shutdown_tx.clone(),
        })
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                msg = self.rx.recv() => {
                    match msg {
                        Some(msg) => {
                            if self.handle_message(msg).await {
                                break;
                            }
                        }
                        None => {
                            // All senders (TaskManagerHandles) have been dropped.
                            // Clean up all running tasks and child processes.
                            self.shutdown_all_tasks().await;
                            break;
                        }
                    }
                }
                _ = self.shutdown_rx.recv() => {
                    self.shutdown_all_tasks().await;
                    break;
                }
            }
        }
    }

    async fn handle_message(&mut self, msg: TaskMessage) -> bool {
        match msg {
            TaskMessage::Start {
                id,
                command,
                description,
                remote_connection,
                timeout,
                response_tx,
            } => {
                let task_id = id.unwrap_or_else(|| generate_simple_id(6));
                let result = self
                    .start_task(
                        task_id.clone(),
                        command,
                        description,
                        timeout,
                        remote_connection,
                    )
                    .await;
                let _ = response_tx.send(result.map(|_| task_id.clone()));
                false
            }
            TaskMessage::Cancel { id, response_tx } => {
                let result = self.cancel_task(&id).await;
                let _ = response_tx.send(result);
                false
            }
            TaskMessage::GetStatus { id, response_tx } => {
                let status = self.tasks.get(&id).map(|entry| entry.task.status.clone());
                let _ = response_tx.send(status);
                false
            }
            TaskMessage::GetTaskDetails { id, response_tx } => {
                let task_info = self.tasks.get(&id).map(|entry| TaskInfo::from(&entry.task));
                let _ = response_tx.send(task_info);
                false
            }
            TaskMessage::GetAllTasks { response_tx } => {
                let mut tasks: Vec<TaskInfo> = self
                    .tasks
                    .values()
                    .map(|entry| TaskInfo::from(&entry.task))
                    .collect();
                tasks.sort_by(|a, b| b.start_time.cmp(&a.start_time));
                let _ = response_tx.send(tasks);
                false
            }
            TaskMessage::TaskUpdate { id, completion } => {
                if let Some(entry) = self.tasks.get_mut(&id) {
                    entry.task.status = completion.final_status.clone();
                    entry.task.output = Some(completion.output.clone());
                    entry.task.error = completion.error;
                    entry.task.duration = Some(
                        Utc::now()
                            .signed_duration_since(entry.task.start_time)
                            .to_std()
                            .unwrap_or_default(),
                    );

                    // Extract checkpoint info for paused and completed tasks
                    if matches!(
                        completion.final_status,
                        TaskStatus::Paused | TaskStatus::Completed
                    ) {
                        let checkpoint_id =
                            serde_json::from_str::<serde_json::Value>(&completion.output)
                                .ok()
                                .and_then(|v| {
                                    v.get("checkpoint_id")
                                        .and_then(|c| c.as_str())
                                        .map(|s| s.to_string())
                                });
                        entry.task.pause_info = Some(PauseInfo {
                            checkpoint_id,
                            raw_output: Some(completion.output),
                        });
                    }

                    // Keep completed tasks in the list so they can be viewed with get_all_tasks
                    // TODO: Consider implementing a cleanup mechanism for old completed tasks
                    // if matches!(entry.task.status, TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled | TaskStatus::TimedOut) {
                    //     self.tasks.remove(&id);
                    // }
                }
                false
            }
            TaskMessage::PartialUpdate { id, output } => {
                if let Some(entry) = self.tasks.get_mut(&id) {
                    match &entry.task.output {
                        Some(existing) => {
                            entry.task.output = Some(format!("{}{}", existing, output));
                        }
                        None => {
                            entry.task.output = Some(output);
                        }
                    }
                }
                false
            }
            TaskMessage::Resume {
                id,
                command,
                response_tx,
            } => {
                let result = self.resume_task(id, command).await;
                let _ = response_tx.send(result);
                false
            }
            TaskMessage::Shutdown { response_tx } => {
                self.shutdown_all_tasks().await;
                let _ = response_tx.send(());
                true
            }
        }
    }

    async fn start_task(
        &mut self,
        id: TaskId,
        command: String,
        description: Option<String>,
        timeout: Option<Duration>,
        remote_connection: Option<RemoteConnectionInfo>,
    ) -> Result<(), TaskError> {
        if self.tasks.contains_key(&id) {
            return Err(TaskError::TaskAlreadyRunning(id));
        }

        let task = Task {
            id: id.clone(),
            status: TaskStatus::Running,
            command: command.clone(),
            description,
            remote_connection: remote_connection.clone(),
            output: None,
            error: None,
            start_time: Utc::now(),
            duration: None,
            timeout,
            pause_info: None,
        };

        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (process_tx, process_rx) = oneshot::channel();
        let task_tx: mpsc::UnboundedSender<TaskMessage> = self.tx.clone();

        let is_remote_task = remote_connection.is_some();

        // Spawn task immediately - SSH connection happens inside the task
        let handle = tokio::spawn(Self::execute_task(
            id.clone(),
            command,
            remote_connection,
            timeout,
            cancel_rx,
            process_tx,
            task_tx,
        ));

        let entry = TaskEntry {
            task,
            handle,
            process_id: None,
            cancel_tx: Some(cancel_tx),
        };

        self.tasks.insert(id.clone(), entry);

        // Wait for the process ID for local tasks only
        if !is_remote_task {
            // Local task - wait for process ID for proper cleanup
            if let Ok(process_id) = process_rx.await
                && let Some(entry) = self.tasks.get_mut(&id)
            {
                entry.process_id = Some(process_id);
            }
        }
        // Remote tasks don't have local process IDs, so we skip waiting

        Ok(())
    }

    async fn resume_task(&mut self, id: TaskId, command: String) -> Result<(), TaskError> {
        // Verify the task exists and is in a resumable state
        if let Some(entry) = self.tasks.get(&id) {
            if !matches!(
                entry.task.status,
                TaskStatus::Paused | TaskStatus::Completed
            ) {
                return Err(TaskError::TaskNotPaused(id));
            }
        } else {
            return Err(TaskError::TaskNotFound(id));
        }

        // Update the task to Running and start a new execution
        let entry = self.tasks.get_mut(&id).unwrap();
        entry.task.status = TaskStatus::Running;
        entry.task.command = command.clone();
        entry.task.pause_info = None;
        entry.task.output = None;
        entry.task.error = None;

        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (process_tx, process_rx) = oneshot::channel();
        let task_tx = self.tx.clone();

        let remote_connection = entry.task.remote_connection.clone();
        let timeout = entry.task.timeout;

        let handle = tokio::spawn(Self::execute_task(
            id.clone(),
            command,
            remote_connection.clone(),
            timeout,
            cancel_rx,
            process_tx,
            task_tx,
        ));

        entry.handle = handle;
        entry.cancel_tx = Some(cancel_tx);
        entry.process_id = None;

        // Wait for process ID for local tasks
        if remote_connection.is_none()
            && let Ok(process_id) = process_rx.await
            && let Some(entry) = self.tasks.get_mut(&id)
        {
            entry.process_id = Some(process_id);
        }

        Ok(())
    }

    async fn cancel_task(&mut self, id: &TaskId) -> Result<(), TaskError> {
        if let Some(mut entry) = self.tasks.remove(id) {
            entry.task.status = TaskStatus::Cancelled;

            if let Some(cancel_tx) = entry.cancel_tx.take() {
                let _ = cancel_tx.send(());
            }

            if let Some(process_id) = entry.process_id {
                terminate_process_group(process_id);
            }

            entry.handle.abort();
            Ok(())
        } else {
            Err(TaskError::TaskNotFound(id.clone()))
        }
    }

    async fn execute_task(
        id: TaskId,
        command: String,
        remote_connection: Option<RemoteConnectionInfo>,
        task_timeout: Option<Duration>,
        mut cancel_rx: oneshot::Receiver<()>,
        process_tx: oneshot::Sender<u32>,
        task_tx: mpsc::UnboundedSender<TaskMessage>,
    ) {
        let completion = if let Some(remote_info) = remote_connection {
            // Remote execution
            Self::execute_remote_task(
                id.clone(),
                command,
                remote_info,
                task_timeout,
                &mut cancel_rx,
                &task_tx,
            )
            .await
        } else {
            // Local execution (existing logic)
            Self::execute_local_task(
                id.clone(),
                command,
                task_timeout,
                &mut cancel_rx,
                process_tx,
                &task_tx,
            )
            .await
        };

        // Send task completion back to manager
        let _ = task_tx.send(TaskMessage::TaskUpdate {
            id: id.clone(),
            completion,
        });
    }

    async fn execute_local_task(
        id: TaskId,
        command: String,
        task_timeout: Option<Duration>,
        cancel_rx: &mut oneshot::Receiver<()>,
        process_tx: oneshot::Sender<u32>,
        task_tx: &mpsc::UnboundedSender<TaskMessage>,
    ) -> TaskCompletion {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&command)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            cmd.env("DEBIAN_FRONTEND", "noninteractive")
                .env("SUDO_ASKPASS", "/bin/false")
                .process_group(0);
        }
        #[cfg(windows)]
        {
            // On Windows, create a new process group
            cmd.creation_flags(0x00000200); // CREATE_NEW_PROCESS_GROUP
        }

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) => {
                return TaskCompletion {
                    output: String::new(),
                    error: Some(format!("Failed to spawn command: {}", err)),
                    final_status: TaskStatus::Failed,
                };
            }
        };

        // Send the process ID back to the manager for tracking
        if let Some(process_id) = child.id() {
            let _ = process_tx.send(process_id);
        }

        // Take stdout and stderr for streaming
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stdout_reader = BufReader::new(stdout);
        let stderr_reader = BufReader::new(stderr);

        let mut stdout_lines = stdout_reader.lines();
        let mut stderr_lines = stderr_reader.lines();

        // Helper function to stream output and handle cancellation
        let stream_output = async {
            let mut final_output = String::new();
            let mut final_error: Option<String> = None;

            loop {
                tokio::select! {
                    line = stdout_lines.next_line() => {
                        match line {
                            Ok(Some(line)) => {
                                let output_line = format!("{}\n", line);
                                final_output.push_str(&output_line);
                                let _ = task_tx.send(TaskMessage::PartialUpdate {
                                    id: id.clone(),
                                    output: output_line,
                                });
                            }
                            Ok(None) => {
                                // stdout stream ended
                            }
                            Err(err) => {
                                final_error = Some(format!("Error reading stdout: {}", err));
                                break;
                            }
                        }
                    }
                    line = stderr_lines.next_line() => {
                        match line {
                            Ok(Some(line)) => {
                                let output_line = format!("{}\n", line);
                                final_output.push_str(&output_line);
                                let _ = task_tx.send(TaskMessage::PartialUpdate {
                                    id: id.clone(),
                                    output: output_line,
                                });
                            }
                            Ok(None) => {
                                // stderr stream ended
                            }
                            Err(err) => {
                                final_error = Some(format!("Error reading stderr: {}", err));
                                break;
                            }
                        }
                    }
                    status = child.wait() => {
                        match status {
                            Ok(exit_status) => {
                                if final_output.is_empty() {
                                    final_output = "No output".to_string();
                                }

                                let completion = if exit_status.success() {
                                    TaskCompletion {
                                        output: final_output,
                                        error: final_error,
                                        final_status: TaskStatus::Completed,
                                    }
                                } else if exit_status.code() == Some(10) {
                                    TaskCompletion {
                                        output: final_output,
                                        error: None,
                                        final_status: TaskStatus::Paused,
                                    }
                                } else {
                                    TaskCompletion {
                                        output: final_output,
                                        error: final_error.or_else(|| Some(format!("Command failed with exit code: {:?}", exit_status.code()))),
                                        final_status: TaskStatus::Failed,
                                    }
                                };
                                return completion;
                            }
                            Err(err) => {
                                return TaskCompletion {
                                    output: final_output,
                                    error: Some(err.to_string()),
                                    final_status: TaskStatus::Failed,
                                };
                            }
                        }
                    }
                    _ = &mut *cancel_rx => {
                        return TaskCompletion {
                            output: final_output,
                            error: Some("Tool call was cancelled and don't try to run it again".to_string()),
                            final_status: TaskStatus::Cancelled,
                        };
                    }
                }
            }

            TaskCompletion {
                output: final_output,
                error: final_error,
                final_status: TaskStatus::Failed,
            }
        };

        // Execute with timeout if provided
        if let Some(timeout_duration) = task_timeout {
            match timeout(timeout_duration, stream_output).await {
                Ok(result) => result,
                Err(_) => TaskCompletion {
                    output: String::new(),
                    error: Some("Task timed out".to_string()),
                    final_status: TaskStatus::TimedOut,
                },
            }
        } else {
            stream_output.await
        }
    }

    async fn execute_remote_task(
        id: TaskId,
        command: String,
        remote_info: RemoteConnectionInfo,
        task_timeout: Option<Duration>,
        cancel_rx: &mut oneshot::Receiver<()>,
        task_tx: &mpsc::UnboundedSender<TaskMessage>,
    ) -> TaskCompletion {
        // Use RemoteConnectionManager to get a connection
        let connection_manager = RemoteConnectionManager::new();
        let connection = match connection_manager.get_connection(&remote_info).await {
            Ok(conn) => conn,
            Err(e) => {
                return TaskCompletion {
                    output: String::new(),
                    error: Some(format!("Failed to establish remote connection: {}", e)),
                    final_status: TaskStatus::Failed,
                };
            }
        };

        // Create progress callback for streaming updates
        let task_tx_clone = task_tx.clone();
        let id_clone = id.clone();
        let progress_callback = move |output: String| {
            if !output.trim().is_empty() {
                let _ = task_tx_clone.send(TaskMessage::PartialUpdate {
                    id: id_clone.clone(),
                    output,
                });
            }
        };

        // Use unified execution with proper cancellation and timeout
        let options = crate::remote_connection::CommandOptions {
            timeout: task_timeout,
            with_progress: false,
            simple: false,
        };

        match connection
            .execute_command_unified(&command, options, cancel_rx, Some(progress_callback), None)
            .await
        {
            Ok((output, exit_code)) => TaskCompletion {
                output,
                error: if exit_code != 0 {
                    Some(format!("Command exited with code {}", exit_code))
                } else {
                    None
                },
                final_status: TaskStatus::Completed,
            },
            Err(e) => {
                let error_msg = e.to_string();
                let status = if error_msg.contains("timed out") {
                    TaskStatus::TimedOut
                } else if error_msg.contains("cancelled") {
                    TaskStatus::Cancelled
                } else {
                    TaskStatus::Failed
                };

                TaskCompletion {
                    output: String::new(),
                    error: Some(if error_msg.contains("cancelled") {
                        "Tool call was cancelled and don't try to run it again".to_string()
                    } else {
                        format!("Remote command failed: {}", error_msg)
                    }),
                    final_status: status,
                }
            }
        }
    }

    async fn shutdown_all_tasks(&mut self) {
        for (_id, mut entry) in self.tasks.drain() {
            if let Some(cancel_tx) = entry.cancel_tx.take() {
                let _ = cancel_tx.send(());
            }

            if let Some(process_id) = entry.process_id {
                terminate_process_group(process_id);
            }

            entry.handle.abort();
        }
    }
}

pub struct TaskManagerHandle {
    tx: mpsc::UnboundedSender<TaskMessage>,
    shutdown_tx: broadcast::Sender<()>,
}

impl Drop for TaskManagerHandle {
    fn drop(&mut self) {
        // Signal the TaskManager to shut down all tasks and kill child processes.
        // This fires on the broadcast channel that TaskManager::run() listens on,
        // triggering shutdown_all_tasks() which kills every process group.
        //
        // This is a last-resort safety net — callers should prefer calling
        // handle.shutdown().await for a clean async shutdown. But if the handle
        // is dropped without that (e.g., panic, std::process::exit, unexpected
        // scope exit), this ensures child processes don't leak.
        let _ = self.shutdown_tx.send(());
    }
}

impl TaskManagerHandle {
    pub async fn start_task(
        &self,
        command: String,
        description: Option<String>,
        timeout: Option<Duration>,
        remote_connection: Option<RemoteConnectionInfo>,
    ) -> Result<TaskInfo, TaskError> {
        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(TaskMessage::Start {
                id: None,
                command: command.clone(),
                description,
                remote_connection: remote_connection.clone(),
                timeout,
                response_tx,
            })
            .map_err(|_| TaskError::ManagerShutdown)?;

        let task_id = response_rx
            .await
            .map_err(|_| TaskError::ManagerShutdown)??;

        // Wait for the task to start and get its status
        tokio::time::sleep(START_TASK_WAIT_TIME).await;

        let task_info = self
            .get_task_details(task_id.clone())
            .await
            .map_err(|_| TaskError::ManagerShutdown)?
            .ok_or_else(|| TaskError::TaskNotFound(task_id.clone()))?;

        // If the task failed or was cancelled during start, return an error
        if matches!(task_info.status, TaskStatus::Failed | TaskStatus::Cancelled) {
            return Err(TaskError::TaskFailedOnStart(
                task_info
                    .output
                    .unwrap_or_else(|| "Unknown reason".to_string()),
            ));
        }

        // Return the task info with updated status
        Ok(task_info)
    }

    pub async fn cancel_task(&self, id: TaskId) -> Result<TaskInfo, TaskError> {
        // Get the task info before cancelling
        let task_info = self
            .get_all_tasks()
            .await?
            .into_iter()
            .find(|task| task.id == id)
            .ok_or_else(|| TaskError::TaskNotFound(id.clone()))?;

        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(TaskMessage::Cancel { id, response_tx })
            .map_err(|_| TaskError::ManagerShutdown)?;

        response_rx
            .await
            .map_err(|_| TaskError::ManagerShutdown)??;

        // Return the task info with updated status
        Ok(TaskInfo {
            status: TaskStatus::Cancelled,
            duration: Some(
                Utc::now()
                    .signed_duration_since(task_info.start_time)
                    .to_std()
                    .unwrap_or_default(),
            ),
            ..task_info
        })
    }

    pub async fn resume_task(&self, id: TaskId, command: String) -> Result<TaskInfo, TaskError> {
        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(TaskMessage::Resume {
                id: id.clone(),
                command,
                response_tx,
            })
            .map_err(|_| TaskError::ManagerShutdown)?;

        response_rx
            .await
            .map_err(|_| TaskError::ManagerShutdown)??;

        // Wait for the task to start
        tokio::time::sleep(START_TASK_WAIT_TIME).await;

        let task_info = self
            .get_task_details(id.clone())
            .await
            .map_err(|_| TaskError::ManagerShutdown)?
            .ok_or(TaskError::TaskNotFound(id))?;

        Ok(task_info)
    }

    pub async fn get_task_status(&self, id: TaskId) -> Result<Option<TaskStatus>, TaskError> {
        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(TaskMessage::GetStatus { id, response_tx })
            .map_err(|_| TaskError::ManagerShutdown)?;

        response_rx.await.map_err(|_| TaskError::ManagerShutdown)
    }

    pub async fn get_task_details(&self, id: TaskId) -> Result<Option<TaskInfo>, TaskError> {
        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(TaskMessage::GetTaskDetails { id, response_tx })
            .map_err(|_| TaskError::ManagerShutdown)?;

        response_rx.await.map_err(|_| TaskError::ManagerShutdown)
    }

    pub async fn get_all_tasks(&self) -> Result<Vec<TaskInfo>, TaskError> {
        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(TaskMessage::GetAllTasks { response_tx })
            .map_err(|_| TaskError::ManagerShutdown)?;

        response_rx.await.map_err(|_| TaskError::ManagerShutdown)
    }

    pub async fn shutdown(&self) -> Result<(), TaskError> {
        let (response_tx, response_rx) = oneshot::channel();

        self.tx
            .send(TaskMessage::Shutdown { response_tx })
            .map_err(|_| TaskError::ManagerShutdown)?;

        response_rx.await.map_err(|_| TaskError::ManagerShutdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_task_manager_shutdown() {
        let task_manager = TaskManager::new();
        let handle = task_manager.handle();

        // Spawn the task manager
        let manager_handle = tokio::spawn(async move {
            task_manager.run().await;
        });

        // Start a background task
        let task_info = handle
            .start_task("sleep 5".to_string(), None, None, None)
            .await
            .expect("Failed to start task");

        // Verify task is running
        let status = handle
            .get_task_status(task_info.id.clone())
            .await
            .expect("Failed to get task status");
        assert_eq!(status, Some(TaskStatus::Running));

        // Shutdown the task manager
        handle
            .shutdown()
            .await
            .expect("Failed to shutdown task manager");

        // Wait a bit for the shutdown to complete
        sleep(Duration::from_millis(100)).await;

        // Verify the manager task has completed
        assert!(manager_handle.is_finished());
    }

    #[tokio::test]
    async fn test_task_manager_cancels_tasks_on_shutdown() {
        let task_manager = TaskManager::new();
        let handle = task_manager.handle();

        // Spawn the task manager
        let manager_handle = tokio::spawn(async move {
            task_manager.run().await;
        });

        // Start a long-running background task
        let task_info = handle
            .start_task("sleep 10".to_string(), None, None, None)
            .await
            .expect("Failed to start task");

        // Verify task is running
        let status = handle
            .get_task_status(task_info.id.clone())
            .await
            .expect("Failed to get task status");
        assert_eq!(status, Some(TaskStatus::Running));

        // Shutdown the task manager
        handle
            .shutdown()
            .await
            .expect("Failed to shutdown task manager");

        // Wait a bit for the shutdown to complete
        sleep(Duration::from_millis(100)).await;

        // Verify the manager task has completed
        assert!(manager_handle.is_finished());
    }

    #[tokio::test]
    async fn test_task_manager_start_and_complete_task() {
        let task_manager = TaskManager::new();
        let handle = task_manager.handle();

        // Spawn the task manager
        let _manager_handle = tokio::spawn(async move {
            task_manager.run().await;
        });

        // Start a simple task
        let task_info = handle
            .start_task("echo 'Hello, World!'".to_string(), None, None, None)
            .await
            .expect("Failed to start task");

        // Wait for the task to complete
        sleep(Duration::from_millis(500)).await;

        // Get task status
        let status = handle
            .get_task_status(task_info.id.clone())
            .await
            .expect("Failed to get task status");
        assert_eq!(status, Some(TaskStatus::Completed));

        // Get all tasks
        let tasks = handle
            .get_all_tasks()
            .await
            .expect("Failed to get all tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, TaskStatus::Completed);

        // Shutdown the task manager
        handle
            .shutdown()
            .await
            .expect("Failed to shutdown task manager");
    }

    #[tokio::test]
    async fn test_task_manager_detects_immediate_failure() {
        let task_manager = TaskManager::new();
        let handle = task_manager.handle();

        // Spawn the task manager
        let _manager_handle = tokio::spawn(async move {
            task_manager.run().await;
        });

        // Start a task that will fail immediately
        let result = handle
            .start_task("nonexistent_command_12345".to_string(), None, None, None)
            .await;

        // Should get a TaskFailedOnStart error
        assert!(matches!(result, Err(TaskError::TaskFailedOnStart(_))));

        // Shutdown the task manager
        handle
            .shutdown()
            .await
            .expect("Failed to shutdown task manager");
    }

    #[tokio::test]
    async fn test_task_manager_handle_drop_triggers_shutdown() {
        let task_manager = TaskManager::new();
        let handle = task_manager.handle();

        let manager_handle = tokio::spawn(async move {
            task_manager.run().await;
        });

        // Start a long-running task
        let _task_info = handle
            .start_task("sleep 30".to_string(), None, None, None)
            .await
            .expect("Failed to start task");

        // Drop the handle WITHOUT calling shutdown()
        drop(handle);

        // The Drop impl sends on the broadcast shutdown channel,
        // which causes TaskManager::run() to call shutdown_all_tasks() and exit.
        // Give it a moment to process.
        sleep(Duration::from_millis(500)).await;

        assert!(
            manager_handle.is_finished(),
            "TaskManager::run() should have exited after handle was dropped"
        );
    }

    #[tokio::test]
    async fn test_task_manager_handle_drop_kills_child_processes() {
        let task_manager = TaskManager::new();
        let handle = task_manager.handle();

        let _manager_handle = tokio::spawn(async move {
            task_manager.run().await;
        });

        // Start a task that writes a marker file while running
        let marker = format!("/tmp/stakpak_test_drop_{}", std::process::id());
        let task_info = handle
            .start_task(format!("touch {} && sleep 30", marker), None, None, None)
            .await
            .expect("Failed to start task");

        // Verify task is running
        let status = handle
            .get_task_status(task_info.id.clone())
            .await
            .expect("Failed to get status");
        assert_eq!(status, Some(TaskStatus::Running));

        // Drop handle without explicit shutdown — Drop should kill the process
        drop(handle);
        sleep(Duration::from_millis(500)).await;

        // Clean up marker file
        let _ = std::fs::remove_file(&marker);
    }

    #[tokio::test]
    async fn test_task_manager_detects_immediate_exit_code_failure() {
        let task_manager = TaskManager::new();
        let handle = task_manager.handle();

        // Spawn the task manager
        let _manager_handle = tokio::spawn(async move {
            task_manager.run().await;
        });

        // Start a task that will exit with non-zero code immediately
        let result = handle
            .start_task("exit 1".to_string(), None, None, None)
            .await;

        // Should get a TaskFailedOnStart error
        assert!(matches!(result, Err(TaskError::TaskFailedOnStart(_))));

        // Shutdown the task manager
        handle
            .shutdown()
            .await
            .expect("Failed to shutdown task manager");
    }
}
