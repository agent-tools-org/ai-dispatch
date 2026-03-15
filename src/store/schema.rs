// Schema helpers and row mappers for the store.
// Exports: create_tables, row_to_task, row_to_event.
// Deps: rusqlite, chrono, crate::types.

use anyhow::Result;
use chrono::{DateTime, Local};
use rusqlite::Row;

use super::Store;
use crate::types::*;

const CREATE_TABLES_SQL: &str = "CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    agent TEXT NOT NULL,
    prompt TEXT NOT NULL,
    resolved_prompt TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    parent_task_id TEXT,
    workgroup_id TEXT,
    caller_kind TEXT,
    caller_session_id TEXT,
    repo_path TEXT,
    worktree_path TEXT,
    worktree_branch TEXT,
    log_path TEXT,
    output_path TEXT,
    tokens INTEGER,
    duration_ms INTEGER,
    model TEXT,
    cost_usd REAL,
    created_at TEXT NOT NULL,
    completed_at TEXT
);
CREATE TABLE IF NOT EXISTS workgroups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    shared_context TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    timestamp TEXT NOT NULL,
    event_type TEXT NOT NULL,
    detail TEXT NOT NULL,
    metadata TEXT
);
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    memory_type TEXT NOT NULL,
    content TEXT NOT NULL,
    source_task_id TEXT,
    agent TEXT,
    project_path TEXT,
    content_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project_path);
CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
CREATE INDEX IF NOT EXISTS idx_memories_hash ON memories(content_hash);
";

const CREATE_WORKGROUPS_SQL: &str = "CREATE TABLE IF NOT EXISTS workgroups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    shared_context TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);";

const CREATE_MEMORIES_SQL: &str = "CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    memory_type TEXT NOT NULL,
    content TEXT NOT NULL,
    source_task_id TEXT,
    agent TEXT,
    project_path TEXT,
    content_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project_path);
CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
CREATE INDEX IF NOT EXISTS idx_memories_hash ON memories(content_hash);";

pub(super) fn create_tables(store: &Store) -> Result<()> {
    store.db().execute_batch(CREATE_TABLES_SQL)?;
    Ok(())
}

/// Idempotent schema migrations for v0.2 columns
pub(super) fn migrate(store: &Store) -> Result<()> {
    let conn = store.db();
    // Add columns if missing (ALTER TABLE ADD COLUMN is idempotent when wrapped in try)
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN model TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN cost_usd REAL;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN parent_task_id TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN workgroup_id TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN caller_kind TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN caller_session_id TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN agent_session_id TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN repo_path TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN resolved_prompt TEXT;");
    let _ = conn.execute_batch(CREATE_WORKGROUPS_SQL);
    let _ = conn.execute_batch(CREATE_MEMORIES_SQL);
    let _ = conn.execute_batch("ALTER TABLE events ADD COLUMN metadata TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN prompt_tokens INTEGER;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN verify TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN read_only INTEGER NOT NULL DEFAULT 0;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN budget INTEGER NOT NULL DEFAULT 0;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN custom_agent_name TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN verify_status TEXT NOT NULL DEFAULT 'skipped';");
    Ok(())
}

pub(super) fn row_to_task(row: &Row) -> rusqlite::Result<Result<Task>> {
    Ok(Ok(Task {
        id: TaskId(row.get::<_, String>(0)?),
        agent: AgentKind::parse_str(&row.get::<_, String>(1)?).unwrap_or(AgentKind::Custom),
        custom_agent_name: row.get(25).ok().flatten(),
        prompt: row.get(2)?,
        resolved_prompt: row.get(3)?,
        status: TaskStatus::parse_str(&row.get::<_, String>(4)?).unwrap_or(TaskStatus::Pending),
        parent_task_id: row.get(5)?,
        workgroup_id: row.get(6)?,
        caller_kind: row.get(7)?,
        caller_session_id: row.get(8)?,
        agent_session_id: row.get(9)?,
        repo_path: row.get(10)?,
        worktree_path: row.get(11)?,
        worktree_branch: row.get(12)?,
        log_path: row.get(13)?,
        output_path: row.get(14)?,
        tokens: row.get(15)?,
        prompt_tokens: row.get(16)?,
        duration_ms: row.get(17)?,
        model: row.get(18)?,
        cost_usd: row.get(19)?,
        created_at: parse_dt(&row.get::<_, String>(20)?),
        completed_at: row.get::<_, Option<String>>(21)?.map(|s| parse_dt(&s)),
        verify: row.get(22)?,
        verify_status: row.get::<_, Option<String>>(26)?.and_then(|s| VerifyStatus::parse_str(&s)).unwrap_or(VerifyStatus::Skipped),
        read_only: row.get(23)?,
        budget: row.get(24)?,
    }))
}

pub(super) fn row_to_event(row: &Row) -> rusqlite::Result<TaskEvent> {
    let metadata_str: Option<String> = row.get(4)?;
    let metadata = metadata_str.and_then(|s| serde_json::from_str(&s).ok());
    Ok(TaskEvent {
        task_id: TaskId(row.get::<_, String>(0)?),
        timestamp: parse_dt(&row.get::<_, String>(1)?),
        event_kind: EventKind::parse_str(&row.get::<_, String>(2)?)
            .unwrap_or(EventKind::Reasoning),
        detail: row.get(3)?,
        metadata,
    })
}

pub(super) fn row_to_memory(row: &Row) -> rusqlite::Result<Result<Memory>> {
    Ok(Ok(Memory {
        id: MemoryId(row.get::<_, String>(0)?),
        memory_type: MemoryType::parse_str(&row.get::<_, String>(1)?).unwrap_or(MemoryType::Fact),
        content: row.get(2)?,
        source_task_id: row.get(3)?,
        agent: row.get(4)?,
        project_path: row.get(5)?,
        content_hash: row.get(6)?,
        created_at: parse_dt(&row.get::<_, String>(7)?),
        expires_at: row.get::<_, Option<String>>(8)?.map(|s| parse_dt(&s)),
    }))
}

pub(super) fn parse_dt(s: &str) -> DateTime<Local> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now())
}
