use crate::models::*;
use libsql::{Connection, Row};
use std::str::FromStr;
use uuid::Uuid;

#[derive(serde::Deserialize)]
struct SessionRow {
    id: String,
    title: String,
    agent_id: String,
    visibility: String,
    created_at: String,
    updated_at: String,
}

#[derive(serde::Deserialize)]
struct CheckpointRow {
    id: String,
    session_id: String,
    status: String,
    execution_depth: i32,
    parent_id: Option<String>,
    created_at: String,
    updated_at: String,
    state: Option<String>,
}

pub async fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            visibility TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        (),
    )
    .await
    .map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS checkpoints (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            status TEXT NOT NULL,
            execution_depth INTEGER NOT NULL,
            parent_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            state TEXT,
            FOREIGN KEY(session_id) REFERENCES sessions(id),
            FOREIGN KEY(parent_id) REFERENCES checkpoints(id)
        )",
        (),
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn list_sessions(conn: &Connection) -> Result<Vec<AgentSession>, String> {
    let mut rows = conn
        .query("SELECT * FROM sessions ORDER BY updated_at DESC", ())
        .await
        .map_err(|e| e.to_string())?;

    let mut sessions = Vec::new();

    while let Ok(Some(row)) = rows.next().await {
        let session_row: SessionRow = libsql::de::from_row(&row).map_err(|e| e.to_string())?;
        let session_id = Uuid::from_str(&session_row.id).map_err(|e| e.to_string())?;

        let checkpoints = get_session_checkpoints(conn, session_id).await?;

        sessions.push((session_row, checkpoints).try_into()?);
    }

    Ok(sessions)
}

pub async fn get_session(conn: &Connection, session_id: Uuid) -> Result<AgentSession, String> {
    let mut rows = conn
        .query(
            "SELECT * FROM sessions WHERE id = ?",
            [session_id.to_string()],
        )
        .await
        .map_err(|e| e.to_string())?;

    if let Ok(Some(row)) = rows.next().await {
        let session_row: SessionRow = libsql::de::from_row(&row).map_err(|e| e.to_string())?;
        let checkpoints = get_session_checkpoints(conn, session_id).await?;

        Ok((session_row, checkpoints).try_into()?)
    } else {
        Err("Session not found".to_string())
    }
}

async fn get_session_checkpoints(
    conn: &Connection,
    session_id: Uuid,
) -> Result<Vec<AgentCheckpointListItem>, String> {
    let mut rows = conn
        .query(
            "SELECT * FROM checkpoints WHERE session_id = ? ORDER BY created_at ASC",
            [session_id.to_string()],
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut checkpoints = Vec::new();

    while let Ok(Some(row)) = rows.next().await {
        checkpoints.push((&row).try_into()?);
    }

    Ok(checkpoints)
}

impl TryFrom<CheckpointRow> for AgentCheckpointListItem {
    type Error = String;

    fn try_from(row: CheckpointRow) -> Result<Self, Self::Error> {
        Ok(AgentCheckpointListItem {
            id: Uuid::from_str(&row.id).map_err(|e| e.to_string())?,
            status: match row.status.as_str() {
                "RUNNING" => AgentStatus::Running,
                "COMPLETE" => AgentStatus::Complete,
                "BLOCKED" => AgentStatus::Blocked,
                "FAILED" => AgentStatus::Failed,
                _ => AgentStatus::Failed,
            },
            execution_depth: row.execution_depth as usize,
            parent: row.parent_id.map(|pid| AgentParentCheckpoint {
                id: Uuid::from_str(&pid).unwrap_or_default(),
            }),
            created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
                .map_err(|e| e.to_string())?
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339(&row.updated_at)
                .map_err(|e| e.to_string())?
                .with_timezone(&chrono::Utc),
        })
    }
}

impl TryFrom<&Row> for AgentCheckpointListItem {
    type Error = String;

    fn try_from(row: &Row) -> Result<Self, Self::Error> {
        let checkpoint_row: CheckpointRow = libsql::de::from_row(row).map_err(|e| e.to_string())?;
        checkpoint_row.try_into()
    }
}

impl TryFrom<(SessionRow, Vec<AgentCheckpointListItem>)> for AgentSession {
    type Error = String;

    fn try_from(
        (row, checkpoints): (SessionRow, Vec<AgentCheckpointListItem>),
    ) -> Result<Self, Self::Error> {
        Ok(AgentSession {
            id: Uuid::from_str(&row.id).map_err(|e| e.to_string())?,
            title: row.title,
            agent_id: AgentID::from_str(&row.agent_id).map_err(|e| e.to_string())?,
            visibility: match row.visibility.as_str() {
                "PUBLIC" => AgentSessionVisibility::Public,
                _ => AgentSessionVisibility::Private,
            },
            created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
                .map_err(|e| e.to_string())?
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339(&row.updated_at)
                .map_err(|e| e.to_string())?
                .with_timezone(&chrono::Utc),
            checkpoints,
        })
    }
}

pub async fn get_checkpoint(
    conn: &Connection,
    checkpoint_id: Uuid,
) -> Result<RunAgentOutput, String> {
    let mut rows = conn
        .query(
            "SELECT * FROM checkpoints WHERE id = ?",
            [checkpoint_id.to_string()],
        )
        .await
        .map_err(|e| e.to_string())?;

    if let Ok(Some(row)) = rows.next().await {
        let checkpoint_row: CheckpointRow =
            libsql::de::from_row(&row).map_err(|e| e.to_string())?;
        let checkpoint = (&row).try_into()?;

        let session_id = Uuid::from_str(&checkpoint_row.session_id).map_err(|e| e.to_string())?;
        let session = get_session(conn, session_id).await?;

        let state = if let Some(s) = checkpoint_row.state {
            serde_json::from_str(&s).map_err(|e| e.to_string())?
        } else {
            return Err("Checkpoint state not found".to_string());
        };

        Ok(RunAgentOutput {
            checkpoint,
            session: session.into(),
            output: state,
        })
    } else {
        Err("Checkpoint not found".to_string())
    }
}

pub async fn get_latest_checkpoint(
    conn: &Connection,
    session_id: Uuid,
) -> Result<RunAgentOutput, String> {
    let mut rows = conn
        .query(
            "SELECT * FROM checkpoints WHERE session_id = ? ORDER BY created_at DESC LIMIT 1",
            [session_id.to_string()],
        )
        .await
        .map_err(|e| e.to_string())?;

    if let Ok(Some(row)) = rows.next().await {
        let checkpoint_row: CheckpointRow =
            libsql::de::from_row(&row).map_err(|e| e.to_string())?;
        let checkpoint_id = Uuid::from_str(&checkpoint_row.id).map_err(|e| e.to_string())?;
        get_checkpoint(conn, checkpoint_id).await
    } else {
        Err("No checkpoints found for session".to_string())
    }
}

pub async fn create_session(conn: &Connection, session: &AgentSession) -> Result<(), String> {
    conn.execute(
        "INSERT INTO sessions (id, title, agent_id, visibility, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        (
            session.id.to_string(),
            session.title.as_str(),
            match session.agent_id {
                AgentID::PabloV1 => "pablo:v1",
            },
            session.visibility.to_string(),
            session.created_at.to_rfc3339(),
            session.updated_at.to_rfc3339(),
        ),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn create_checkpoint(
    conn: &Connection,
    session_id: Uuid,
    checkpoint: &AgentCheckpointListItem,
    state: &AgentOutput,
) -> Result<(), String> {
    let state_json = serde_json::to_string(state).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO checkpoints (id, session_id, status, execution_depth, parent_id, created_at, updated_at, state) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        (
            checkpoint.id.to_string(),
            session_id.to_string(),
            checkpoint.status.to_string(),
            checkpoint.execution_depth as i32,
            checkpoint.parent.as_ref().map(|p| p.id.to_string()),
            checkpoint.created_at.to_rfc3339(),
            checkpoint.updated_at.to_rfc3339(),
            state_json,
        ),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}
