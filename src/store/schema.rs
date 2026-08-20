// Schema helpers and row mappers for the store.
// Exports: create_tables, row_to_task, row_to_event.
// Deps: rusqlite, chrono, crate::types.
pub(super) use super::schema_rows::{row_to_event, row_to_memory};
use super::{kg_schema::CREATE_KG_SQL, Store};
use crate::types::*;
use anyhow::Result;
use chrono::{DateTime, Local};
use rusqlite::Row;
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
    project_id TEXT,
    worktree_path TEXT,
    effective_dir TEXT,
    worktree_branch TEXT, final_head_sha TEXT, final_branch TEXT,
    start_sha TEXT,
    log_path TEXT,
    output_path TEXT,
    tokens INTEGER,
    duration_ms INTEGER,
    model TEXT,
    cost_usd REAL,
    created_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT,
    completion_summary TEXT,
    peer_review TEXT,
    category TEXT,
    pending_reason TEXT,
    audit_verdict TEXT, audit_report_path TEXT, delivery_assessment TEXT, dispatch_args TEXT,
    declared_difficulty TEXT, declared_budget TEXT, declared_urgency TEXT, declared_rigor TEXT,
    observed_model TEXT,
    attribution_source TEXT,
    principal_merge_override TEXT
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
CREATE TABLE IF NOT EXISTS findings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workgroup_id TEXT NOT NULL,
    content TEXT NOT NULL,
    source_task_id TEXT,
    severity TEXT,
    title TEXT,
    file TEXT,
    lines TEXT,
    category TEXT,
    confidence TEXT,
    verdict TEXT,
    score TEXT,
    note TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_findings_workgroup ON findings(workgroup_id);
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    memory_type TEXT NOT NULL,
    tier TEXT NOT NULL DEFAULT 'on_demand',
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
CREATE TABLE IF NOT EXISTS task_acceptance (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    decision TEXT NOT NULL CHECK(decision IN ('accepted', 'rejected')),
    decided_at TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    source TEXT NOT NULL,
    accepted_head_sha TEXT,
    accepted_branch TEXT,
    artifact_manifest_digest TEXT
);
CREATE INDEX IF NOT EXISTS idx_task_acceptance_task
    ON task_acceptance(task_id, id);
CREATE TABLE IF NOT EXISTS artifact_durability (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    checked_at TEXT NOT NULL,
    accepted_head_sha TEXT NOT NULL,
    manifest_digest TEXT NOT NULL,
    certificate_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_artifact_durability_task
    ON artifact_durability(task_id, id);
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
    tier TEXT NOT NULL DEFAULT 'on_demand',
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
const CREATE_FINDINGS_SQL: &str = "CREATE TABLE IF NOT EXISTS findings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workgroup_id TEXT NOT NULL,
    content TEXT NOT NULL,
    source_task_id TEXT,
    severity TEXT,
    title TEXT,
    file TEXT,
    lines TEXT,
    category TEXT,
    confidence TEXT,
    verdict TEXT,
    score TEXT,
    note TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_findings_workgroup ON findings(workgroup_id);";

pub(super) fn create_tables(store: &Store) -> Result<()> {
    store.db().execute_batch(CREATE_TABLES_SQL)?;
    store.db().execute_batch(CREATE_KG_SQL)?;
    Ok(())
}

/// Idempotent schema migrations for v0.2 columns
pub(super) fn migrate(store: &Store) -> Result<()> {
    let conn = store.db();
    // Add columns if missing (ALTER TABLE ADD COLUMN is idempotent when wrapped in try)
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN model TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN cost_usd REAL;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN started_at TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN exit_code INTEGER;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN parent_task_id TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN workgroup_id TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN caller_kind TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN caller_session_id TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN agent_session_id TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN repo_path TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN resolved_prompt TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN start_sha TEXT;");
    let _ = conn.execute_batch(CREATE_WORKGROUPS_SQL);
    let _ = conn.execute_batch(CREATE_MEMORIES_SQL);
    let _ = conn.execute_batch("ALTER TABLE memories ADD COLUMN supersedes TEXT;");
    let _ =
        conn.execute_batch("ALTER TABLE memories ADD COLUMN version INTEGER NOT NULL DEFAULT 1;");
    let _ = conn
        .execute_batch("ALTER TABLE memories ADD COLUMN inject_count INTEGER NOT NULL DEFAULT 0;");
    let _ = conn.execute_batch("ALTER TABLE memories ADD COLUMN last_injected_at TEXT;");
    let _ = conn
        .execute_batch("ALTER TABLE memories ADD COLUMN success_count INTEGER NOT NULL DEFAULT 0;");
    let _ = conn
        .execute_batch("ALTER TABLE memories ADD COLUMN tier TEXT NOT NULL DEFAULT 'on_demand';");
    let _ = conn.execute_batch("ALTER TABLE events ADD COLUMN metadata TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN prompt_tokens INTEGER;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN verify TEXT;");
    let _ =
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN read_only INTEGER NOT NULL DEFAULT 0;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN budget INTEGER NOT NULL DEFAULT 0;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN custom_agent_name TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN completion_summary TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN peer_review TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN category TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN pending_reason TEXT;");
    let _ = conn.execute_batch(
        "ALTER TABLE tasks ADD COLUMN verify_status TEXT NOT NULL DEFAULT 'skipped';",
    );
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN audit_verdict TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN audit_report_path TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN delivery_assessment TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN dispatch_args TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN final_head_sha TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN final_branch TEXT;");
    let _ = conn.execute_batch(
        "UPDATE tasks
         SET delivery_assessment = verify_status
         WHERE verify_status IN ('empty_diff', 'hollow_output')
           AND delivery_assessment IS NULL;",
    );
    let _ = conn.execute_batch(
        "UPDATE tasks
         SET verify_status = 'skipped'
         WHERE verify_status IN ('empty_diff', 'hollow_output');",
    );
    let _ = conn.execute_batch("ALTER TABLE workgroups ADD COLUMN created_by TEXT;");
    let _ = conn.execute_batch(CREATE_FINDINGS_SQL);
    let _ = conn.execute_batch("ALTER TABLE findings ADD COLUMN severity TEXT;");
    let _ = conn.execute_batch("ALTER TABLE findings ADD COLUMN title TEXT;");
    let _ = conn.execute_batch("ALTER TABLE findings ADD COLUMN file TEXT;");
    let _ = conn.execute_batch("ALTER TABLE findings ADD COLUMN lines TEXT;");
    let _ = conn.execute_batch("ALTER TABLE findings ADD COLUMN category TEXT;");
    let _ = conn.execute_batch("ALTER TABLE findings ADD COLUMN confidence TEXT;");
    let _ = conn.execute_batch("ALTER TABLE findings ADD COLUMN verdict TEXT;");
    let _ = conn.execute_batch("ALTER TABLE findings ADD COLUMN score TEXT;");
    let _ = conn.execute_batch("ALTER TABLE findings ADD COLUMN note TEXT;");
    let _ = conn.execute_batch("ALTER TABLE findings ADD COLUMN updated_at TEXT;");
    let _ = conn.execute_batch(CREATE_KG_SQL);
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN principal_merge_override TEXT;");
    // Performance indexes for hot query paths
    let _ =
        conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at);");
    let _ = conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);");
    let _ = conn
        .execute_batch("CREATE INDEX IF NOT EXISTS idx_tasks_workgroup ON tasks(workgroup_id);");
    let _ = conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_events_task_id ON events(task_id);");
    let _ = conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_events_task_kind ON events(task_id, event_type);",
    );
    super::migrations::migrate_task_messages(&conn)?;
    super::migrations::migrate_declared_task_profile(&conn)?;
    super::migrations::migrate_observed_model(&conn)?;
    super::migrations::migrate_project_id(&conn)?;
    super::migrations::migrate_effective_dir(&conn)?;
    Ok(())
}

pub(super) fn row_to_task(row: &Row) -> rusqlite::Result<Result<Task>> {
    let stored_agent = row.get::<_, String>(1)?;
    let parsed_agent = AgentKind::parse_str(&stored_agent);
    Ok(Ok(Task {
        id: TaskId(row.get::<_, String>(0)?),
        agent: parsed_agent.unwrap_or(AgentKind::Custom),
        // An agent name this binary cannot parse is not a custom agent — it is an
        // agent we do not recognise, and the difference is what the reader needs.
        // A task written by a newer aid (t-8e9194dc, agent `commandcode`) used to
        // render as `custom/unknown/unknown` with the real name discarded, so you
        // could not tell which agent had run. Keep the raw string; a genuine
        // custom agent's own name still wins when the column holds one.
        custom_agent_name: row
            .get::<_, Option<String>>(26)
            .ok()
            .flatten()
            .or_else(|| parsed_agent.is_none().then(|| stored_agent.clone())),
        prompt: row.get(2)?,
        resolved_prompt: row.get(3)?,
        category: row.get(29).ok().flatten(),
        status: TaskStatus::parse_str(&row.get::<_, String>(4)?).unwrap_or(TaskStatus::Pending),
        parent_task_id: row.get(5)?,
        workgroup_id: row.get(6)?,
        caller_kind: row.get(7)?,
        caller_session_id: row.get(8)?,
        agent_session_id: row.get(9)?,
        repo_path: row.get(10)?,
        // project_id is selected by name so historical SELECTs that omit it
        // still map; NULL remains the honest unattributed bucket.
        project_id: row.get("project_id").ok().flatten(),
        worktree_path: row.get(11)?,
        effective_dir: row.get("effective_dir").ok().flatten(),
        worktree_branch: row.get(12)?,
        final_head_sha: row.get(34).ok().flatten(),
        final_branch: row.get(35).ok().flatten(),
        start_sha: row.get(13)?,
        log_path: row.get(14)?,
        output_path: row.get(15)?,
        tokens: row.get(16)?,
        prompt_tokens: row.get(17)?,
        duration_ms: row.get(18)?,
        // Column 19 is still named `model` on disk. Renaming it would move
        // ordinals that the rest of this mapper depends on, and those ordinals
        // already disagree with CREATE_TABLES_SQL's declared order for columns
        // added by ALTER over time. The new column is read by name for that
        // reason, not by position.
        requested_model: row.get(19)?,
        observed_model: row.get("observed_model").ok().flatten(),
        attribution_source: row
            .get::<_, Option<String>>("attribution_source")
            .ok()
            .flatten()
            .and_then(|value| AttributionSource::parse_str(&value)),
        cost_usd: row.get(20)?,
        exit_code: row.get(28).ok().flatten(),
        created_at: parse_dt(&row.get::<_, String>(21)?),
        completed_at: row.get::<_, Option<String>>(22)?.map(|s| parse_dt(&s)),
        verify: row.get(23)?,
        verify_status: row
            .get::<_, Option<String>>(27)?
            .and_then(|s| VerifyStatus::parse_str(&s))
            .unwrap_or(VerifyStatus::Skipped),
        pending_reason: row.get(30).ok().flatten(),
        read_only: row.get(24)?,
        budget: row.get(25)?,
        audit_verdict: row.get(31).ok().flatten(),
        audit_report_path: row.get(32).ok().flatten(),
        delivery_assessment: row
            .get::<_, Option<String>>(33)
            .ok()
            .flatten()
            .and_then(|value| DeliveryAssessment::parse_str(&value)),
    }))
}

pub(super) fn parse_dt(s: &str) -> DateTime<Local> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now())
}
