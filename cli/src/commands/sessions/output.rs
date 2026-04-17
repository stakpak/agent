//! Human + JSON renderers for the `stakpak sessions` subcommands.

use serde::Serialize;
use stakpak_api::{
    BackendInfo, BackendKind, Session, SessionStatus, SessionSummary, SessionVisibility,
};
use stakpak_shared::models::integrations::openai::ChatMessage;
use stakpak_shared::utils::sanitize_text_output;

/// Output format for session commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Human,
    Json,
}

impl OutputMode {
    pub fn from_flag(json: bool) -> Self {
        if json { Self::Json } else { Self::Human }
    }
}

// =============================================================================
// List output
// =============================================================================

pub fn render_list(sessions: &[SessionSummary], backend: &BackendInfo, mode: OutputMode) -> String {
    match mode {
        OutputMode::Json => render_list_json(sessions, backend),
        OutputMode::Human => render_list_human(sessions, backend),
    }
}

#[derive(Debug, Serialize)]
struct ListOutput<'a> {
    backend: &'a BackendInfo,
    sessions: &'a [SessionSummary],
}

fn render_list_json(sessions: &[SessionSummary], backend: &BackendInfo) -> String {
    let out = ListOutput { backend, sessions };
    serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
}

fn render_list_human(sessions: &[SessionSummary], backend: &BackendInfo) -> String {
    let mut out = format!("{}\n", render_backend_header(backend));
    if sessions.is_empty() {
        out.push_str("No sessions found.");
        return out;
    }

    // Column widths
    let id_w = 36;
    let msgs_w = 5;
    let time_w = 20;
    let titles: Vec<String> = sessions
        .iter()
        .map(|s| {
            let sanitized = sanitize_text_output(&s.title);
            truncate(&collapse_whitespace(&sanitized), 50)
        })
        .collect();
    let title_w = titles
        .iter()
        .map(|t| t.chars().count())
        .max()
        .unwrap_or(5)
        .max(5);

    out.push_str(&format!(
        "{:<id_w$}  {:<title_w$}  {:>msgs_w$}  {:<time_w$}\n",
        "ID",
        "TITLE",
        "MSGS",
        "LAST ACTIVITY",
        id_w = id_w,
        title_w = title_w,
        msgs_w = msgs_w,
        time_w = time_w,
    ));
    out.push_str(&format!(
        "{}  {}  {}  {}\n",
        "-".repeat(id_w),
        "-".repeat(title_w),
        "-".repeat(msgs_w),
        "-".repeat(time_w),
    ));

    for (s, title) in sessions.iter().zip(titles.iter()) {
        let last = s
            .last_message_at
            .unwrap_or(s.updated_at)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        out.push_str(&format!(
            "{:<id_w$}  {:<title_w$}  {:>msgs_w$}  {:<time_w$}\n",
            s.id,
            title,
            s.message_count,
            last,
            id_w = id_w,
            title_w = title_w,
            msgs_w = msgs_w,
            time_w = time_w,
        ));
    }

    out
}

// =============================================================================
// Show output
// =============================================================================

#[derive(Debug, Serialize)]
pub struct ShowOutput<'a> {
    pub id: uuid::Uuid,
    pub title: &'a str,
    pub status: SessionStatus,
    pub visibility: SessionVisibility,
    pub cwd: Option<&'a str>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub active_checkpoint_id: Option<uuid::Uuid>,
    pub messages: &'a [ChatMessage],
    pub backend: &'a BackendInfo,
}

pub fn render_show(
    session: &Session,
    messages: &[ChatMessage],
    backend: &BackendInfo,
    profile: Option<&str>,
    mode: OutputMode,
) -> String {
    match mode {
        OutputMode::Json => render_show_json(session, messages, backend),
        OutputMode::Human => render_show_human(session, messages, backend, profile),
    }
}

fn render_show_json(session: &Session, messages: &[ChatMessage], backend: &BackendInfo) -> String {
    let out = ShowOutput {
        id: session.id,
        title: &session.title,
        status: session.status,
        visibility: session.visibility,
        cwd: session.cwd.as_deref(),
        created_at: session.created_at,
        updated_at: session.updated_at,
        active_checkpoint_id: session.active_checkpoint.as_ref().map(|c| c.id),
        messages,
        backend,
    };
    serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
}

fn render_show_human(
    session: &Session,
    messages: &[ChatMessage],
    backend: &BackendInfo,
    profile: Option<&str>,
) -> String {
    let mut out = format!("{}\n", render_backend_header(backend));
    out.push_str(&format!("ID:          {}\n", session.id));
    let sanitized_title = sanitize_text_output(&session.title);
    out.push_str(&format!(
        "Title:       {}\n",
        collapse_whitespace(&sanitized_title)
    ));
    out.push_str(&format!("Status:      {}\n", session.status));
    out.push_str(&format!("Visibility:  {}\n", session.visibility));
    if let Some(cwd) = &session.cwd {
        out.push_str(&format!("Working dir: {}\n", sanitize_text_output(cwd)));
    }
    out.push_str(&format!(
        "Created:     {}\n",
        session.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    ));
    out.push_str(&format!(
        "Updated:     {}\n",
        session.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    ));
    if let Some(cp) = &session.active_checkpoint {
        out.push_str(&format!("Checkpoint:  {}\n", cp.id));
    } else {
        out.push_str("Checkpoint:  (none)\n");
    }

    let resume = match profile {
        Some(p) if !p.is_empty() && p != "default" => {
            format!("stakpak --profile {} --session {}", p, session.id)
        }
        _ => format!("stakpak --session {}", session.id),
    };
    out.push_str(&format!("\nResume: {}\n", resume));

    out.push_str(&format!("\nMessages ({}):\n", messages.len()));
    if messages.is_empty() {
        out.push_str("  (no messages)\n");
        return out;
    }

    for (i, m) in messages.iter().enumerate() {
        out.push_str(&format!("\n[{}] {}\n", i + 1, m.role));
        if let Some(content) = &m.content {
            let text = sanitize_text_output(&content.to_string());
            for line in text.lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
        if let Some(tool_calls) = &m.tool_calls {
            for tc in tool_calls {
                out.push_str(&format!(
                    "    [tool_call] {} ({})\n",
                    sanitize_text_output(&tc.function.name),
                    sanitize_text_output(&tc.id),
                ));
            }
        }
        if let Some(tc_id) = &m.tool_call_id {
            out.push_str(&format!(
                "    [tool_call_id] {}\n",
                sanitize_text_output(tc_id)
            ));
        }
    }

    out
}

// =============================================================================
// Errors
// =============================================================================

#[derive(Debug, Serialize)]
struct ErrorJson<'a> {
    error: &'a str,
    code: &'a str,
}

pub fn render_error(message: &str, code: &str, mode: OutputMode) -> String {
    match mode {
        OutputMode::Json => {
            let e = ErrorJson {
                error: message,
                code,
            };
            serde_json::to_string(&e)
                .unwrap_or_else(|_| format!(r#"{{"error": "{}", "code": "{}"}}"#, message, code))
        }
        OutputMode::Human => format!("Error: {}", message),
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn render_backend_header(backend: &BackendInfo) -> String {
    match (
        &backend.kind,
        &backend.profile,
        &backend.endpoint,
        &backend.store_path,
    ) {
        (BackendKind::StakpakApi, Some(profile), Some(endpoint), _) => {
            format!("Backend: stakpak-api (profile: {profile}, endpoint: {endpoint})")
        }
        (BackendKind::StakpakApi, None, Some(endpoint), _) => {
            format!("Backend: stakpak-api (endpoint: {endpoint})")
        }
        (BackendKind::Local, _, _, Some(store_path)) => {
            format!("Backend: local ({store_path})")
        }
        _ => "Backend: unknown".to_string(),
    }
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_space = false;

    for ch in s.chars() {
        let normalized = match ch {
            '\r' | '\n' | '\t' => ' ',
            other => other,
        };

        if normalized == ' ' {
            if !last_was_space && !out.is_empty() {
                out.push(' ');
                last_was_space = true;
            }
            continue;
        }

        out.push(normalized);
        last_was_space = false;
    }

    if out.ends_with(' ') {
        out.pop();
    }

    out
}

fn truncate(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    // Find the byte offset of the (max_chars)-th character. If it exists,
    // the string is longer than `max_chars` chars and needs truncation.
    if let Some((byte_idx, _)) = s.char_indices().nth(max_chars) {
        let cut = s
            .char_indices()
            .nth(max_chars - 1)
            .map(|(i, _)| i)
            .unwrap_or(byte_idx);
        let mut out = String::with_capacity(cut + 4);
        out.push_str(&s[..cut]);
        out.push('…');
        out
    } else {
        s.to_string()
    }
}
