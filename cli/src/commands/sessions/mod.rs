//! `stakpak sessions` — list and inspect past sessions.
//!
//! Exposes the `SessionStorage` trait through an agent-friendly CLI with
//! explicit `--json` output. Uses `build_agent_client(&config)` so it works
//! with whatever profile backend is configured (SQLite or Stakpak API) and
//! does not depend on the autopilot server.

use std::io::Write;
use std::sync::Arc;

use clap::Subcommand;
use stakpak_api::{AgentClient, ListSessionsQuery, SessionStorage, StakpakConfig, StorageError};
use uuid::Uuid;

use crate::config::AppConfig;

pub mod messages;
pub mod output;

#[cfg(test)]
mod tests;

use messages::{RoleFilter, filter_messages};
use output::{OutputMode, render_error, render_list, render_show};

const DEFAULT_LIST_LIMIT: u32 = 20;

#[derive(Subcommand, PartialEq)]
pub enum SessionsCommands {
    /// List sessions, newest first.
    ///
    /// Uses the Stakpak API when the active profile has an API key, otherwise the local SQLite store; honours `--profile`.
    List {
        /// Filter by substring match on session title
        #[arg(long)]
        search: Option<String>,

        /// Maximum number of sessions to return
        #[arg(long, default_value_t = DEFAULT_LIST_LIMIT)]
        limit: u32,

        /// Skip the first N sessions
        #[arg(long, default_value_t = 0)]
        offset: u32,

        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },

    /// Show a session's metadata and active-checkpoint messages.
    ///
    /// Uses the Stakpak API when the active profile has an API key, otherwise the local SQLite store; honours `--profile`.
    Show {
        /// Full session UUID
        id: String,

        /// Keep only messages with this role
        #[arg(long, value_name = "ROLE")]
        role: Option<String>,

        /// Keep only the last message (combine with `--role` for role-filtered last)
        #[arg(long)]
        last: bool,

        /// Maximum number of messages to show (keeps the most recent)
        #[arg(long)]
        limit: Option<u32>,

        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

impl SessionsCommands {
    pub async fn run(self, config: AppConfig) -> Result<(), String> {
        match self {
            SessionsCommands::List {
                search,
                limit,
                offset,
                json,
            } => {
                let mode = OutputMode::from_flag(json);
                run_list(&config, search, limit, offset, mode).await
            }
            SessionsCommands::Show {
                id,
                role,
                last,
                limit,
                json,
            } => {
                let mode = OutputMode::from_flag(json);
                run_show(&config, &id, role.as_deref(), last, limit, mode).await
            }
        }
    }
}

async fn build_storage(config: &AppConfig) -> Result<Arc<dyn SessionStorage>, String> {
    let stakpak = config.get_stakpak_api_key().map(|api_key| StakpakConfig {
        api_key,
        api_endpoint: config.api_endpoint.clone(),
    });
    AgentClient::build_session_storage(stakpak, None, Some(config.profile_name.clone())).await
}

pub(crate) async fn list_sessions_output(
    client: Arc<dyn SessionStorage>,
    search: Option<String>,
    limit: u32,
    offset: u32,
    mode: OutputMode,
) -> Result<String, StorageError> {
    let mut query = ListSessionsQuery::new()
        .with_limit(limit)
        .with_offset(offset);
    if let Some(s) = search {
        query = query.with_search(s);
    }

    let result = client.list_sessions(&query).await?;
    let backend = client.backend_info();
    Ok(render_list(&result.sessions, &backend, mode))
}

async fn run_list(
    config: &AppConfig,
    search: Option<String>,
    limit: u32,
    offset: u32,
    mode: OutputMode,
) -> Result<(), String> {
    let client = build_storage(config).await?;

    match list_sessions_output(client, search, limit, offset, mode).await {
        Ok(rendered) => {
            emit_stdout(&rendered);
            Ok(())
        }
        Err(e) => exit_with_storage_error(e, mode),
    }
}

pub(crate) async fn show_session_output(
    client: Arc<dyn SessionStorage>,
    session_id: Uuid,
    role_filter: Option<RoleFilter>,
    last: bool,
    limit: Option<u32>,
    profile: Option<&str>,
    mode: OutputMode,
) -> Result<String, StorageError> {
    let session = client.get_session(session_id).await?;
    let raw_messages = session
        .active_checkpoint
        .as_ref()
        .map(|cp| cp.state.messages.clone())
        .unwrap_or_default();
    let messages = filter_messages(raw_messages, role_filter, last, limit);
    let backend = client.backend_info();

    Ok(render_show(&session, &messages, &backend, profile, mode))
}

async fn run_show(
    config: &AppConfig,
    id_str: &str,
    role: Option<&str>,
    last: bool,
    limit: Option<u32>,
    mode: OutputMode,
) -> Result<(), String> {
    let session_id = match Uuid::parse_str(id_str) {
        Ok(id) => id,
        Err(_) => {
            let msg = format!("invalid session id '{}': expected a full UUID", id_str);
            emit_error(&msg, "invalid_argument", mode);
            std::process::exit(2);
        }
    };

    let role_filter = match role {
        Some(r) => match r.parse::<RoleFilter>() {
            Ok(rf) => Some(rf),
            Err(e) => {
                emit_error(&e, "invalid_argument", mode);
                std::process::exit(2);
            }
        },
        None => None,
    };

    let client = build_storage(config).await?;

    match show_session_output(
        client,
        session_id,
        role_filter,
        last,
        limit,
        Some(config.profile_name.as_str()),
        mode,
    )
    .await
    {
        Ok(rendered) => {
            emit_stdout(&rendered);
            Ok(())
        }
        Err(e) => exit_with_storage_error(e, mode),
    }
}

fn emit_stdout(rendered: &str) {
    if rendered.ends_with('\n') {
        print!("{}", rendered);
    } else {
        println!("{}", rendered);
    }
}

fn emit_error(message: &str, code: &str, mode: OutputMode) {
    let rendered = render_error(message, code, mode);
    let stderr = std::io::stderr();
    let mut handle = stderr.lock();
    let _ = writeln!(handle, "{}", rendered);
}

/// Map a `StorageError` to its JSON error `code` string and the process
/// exit code the CLI should terminate with. Pure so it can be unit-tested
/// without invoking `std::process::exit`.
pub(crate) fn classify_storage_error(err: &StorageError) -> (&'static str, i32) {
    match err {
        StorageError::NotFound(_) => ("not_found", 1),
        StorageError::InvalidRequest(_) => ("invalid_request", 2),
        StorageError::Unauthorized(_) => ("unauthorized", 1),
        StorageError::RateLimited(_) => ("rate_limited", 1),
        StorageError::Connection(_) => ("connection_error", 1),
        StorageError::Internal(_) => ("internal_error", 1),
    }
}

fn exit_with_storage_error(err: StorageError, mode: OutputMode) -> ! {
    let (code_str, exit_code) = classify_storage_error(&err);
    emit_error(&err.to_string(), code_str, mode);
    std::process::exit(exit_code);
}
