// Schema helpers and row mappers for the store.
// Exports: create_tables, row_to_task, row_to_event.
// Deps: rusqlite, chrono, crate::types.
use super::kg_schema::CREATE_KG_SQL;
pub(super) use super::schema_rows::{row_to_event, row_to_memory};
use rusqlite::Connection;
pub(super) mod version;
#[cfg(test)]
mod version_tests;
use crate::types::*;
use anyhow::Result;
use chrono::{DateTime, Local};
use rusqlite::Row;
pub(super) use version::ensure_current;
mod sql;
use sql::*;

pub(super) fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(CREATE_TABLES_SQL)?;
    conn.execute_batch(CREATE_KG_SQL)?;
    Ok(())
}

/// Idempotent schema migrations for v0.2 columns
pub(super) fn migrate(conn: &Connection) -> Result<()> {
    migrate_initial_task_columns(conn)?;
    migrate_memory_columns(conn)?;
    migrate_delivery_columns(conn)?;
    migrate_finding_columns(conn)?;
    // Performance indexes for hot query paths
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at);")?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);")?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_tasks_workgroup ON tasks(workgroup_id);")?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_events_task_id ON events(task_id);")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_events_task_timestamp_id ON events(task_id, timestamp DESC, \
            id DESC);",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_events_task_kind ON events(task_id, \
            event_type);",
    )?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_events_task_id_id ON events(task_id, id);")?;
    super::migrations::migrate_task_messages(conn)?;
    super::migrations::migrate_declared_task_profile(conn)?;
    super::migrations::migrate_observed_model(conn)?;
    super::migrations::migrate_project_id(conn)?;
    super::migrations::migrate_effective_dir(conn)?;
    version::add_column(conn, "tasks", "backup_url TEXT")?;

    Ok(())
}

// project_id is selected by name so historical SELECTs that omit it
// still map; NULL remains the honest unattributed bucket.
// Column 19 is still named `model` on disk. Renaming it would move
// ordinals that the rest of this mapper depends on, and those ordinals
// already disagree with CREATE_TABLES_SQL's declared order for columns
// added by ALTER over time. The new column is read by name for that
// reason, not by position.
pub(super) fn row_to_task(row: &Row) -> rusqlite::Result<Result<Task>> {
    let stored_agent = row.get::<_, String>(1)?;
    let parsed_agent = AgentKind::parse_str(&stored_agent);
    Ok(Ok(Task {
        id: TaskId(row.get::<_, String>(0)?),
        agent: parsed_agent.unwrap_or(AgentKind::Custom),
        custom_agent_name: custom_agent_name(row, parsed_agent, &stored_agent),
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
        requested_model: row.get(19)?,
        observed_model: row.get("observed_model").ok().flatten(),
        attribution_source: attribution_source(row),
        cost_usd: row.get(20)?,
        exit_code: row.get(28).ok().flatten(),
        created_at: parse_dt(&row.get::<_, String>(21)?),
        completed_at: row.get::<_, Option<String>>(22)?.map(|s| parse_dt(&s)),
        verify: row.get(23)?,
        verify_status: verify_status(row)?,
        pending_reason: row.get(30).ok().flatten(),
        read_only: row.get(24)?,
        budget: row.get(25)?,
        audit_verdict: row.get(31).ok().flatten(),
        audit_report_path: row.get(32).ok().flatten(),
        delivery_assessment: delivery_assessment(row),
    }))
}

pub(super) fn parse_dt(s: &str) -> DateTime<Local> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now())
}

fn migrate_initial_task_columns(conn: &rusqlite::Connection) -> Result<()> {
    // Check column presence so unexpected ALTER failures propagate.
    version::add_column(conn, "tasks", "model TEXT")?;
    version::add_column(conn, "tasks", "cost_usd REAL")?;
    version::add_column(conn, "tasks", "started_at TEXT")?;
    version::add_column(conn, "tasks", "exit_code INTEGER")?;
    version::add_column(conn, "tasks", "parent_task_id TEXT")?;
    version::add_column(conn, "tasks", "workgroup_id TEXT")?;
    version::add_column(conn, "tasks", "caller_kind TEXT")?;
    version::add_column(conn, "tasks", "caller_session_id TEXT")?;
    version::add_column(conn, "tasks", "agent_session_id TEXT")?;
    version::add_column(conn, "tasks", "repo_path TEXT")?;
    version::add_column(conn, "tasks", "resolved_prompt TEXT")?;
    version::add_column(conn, "tasks", "start_sha TEXT")?;
    Ok(())
}

fn migrate_memory_columns(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(CREATE_WORKGROUPS_SQL)?;
    conn.execute_batch(CREATE_MEMORIES_SQL)?;
    version::add_column(conn, "memories", "supersedes TEXT")?;
    version::add_column(conn, "memories", "version INTEGER NOT NULL DEFAULT 1")?;
    version::add_column(conn, "memories", "inject_count INTEGER NOT NULL DEFAULT 0")?;
    version::add_column(conn, "memories", "last_injected_at TEXT")?;
    version::add_column(conn, "memories", "success_count INTEGER NOT NULL DEFAULT 0")?;
    version::add_column(conn, "memories", "tier TEXT NOT NULL DEFAULT 'on_demand'")?;
    version::add_column(conn, "events", "metadata TEXT")?;
    Ok(())
}

fn migrate_delivery_columns(conn: &rusqlite::Connection) -> Result<()> {
    version::add_column(conn, "tasks", "prompt_tokens INTEGER")?;
    version::add_column(conn, "tasks", "verify TEXT")?;
    version::add_column(conn, "tasks", "read_only INTEGER NOT NULL DEFAULT 0")?;
    version::add_column(conn, "tasks", "budget INTEGER NOT NULL DEFAULT 0")?;
    version::add_column(conn, "tasks", "custom_agent_name TEXT")?;
    version::add_column(conn, "tasks", "completion_summary TEXT")?;
    version::add_column(conn, "tasks", "peer_review TEXT")?;
    version::add_column(conn, "tasks", "category TEXT")?;
    version::add_column(conn, "tasks", "pending_reason TEXT")?;
    version::add_column(
        conn,
        "tasks",
        "verify_status TEXT NOT NULL DEFAULT 'skipped'",
    )?;
    version::add_column(conn, "tasks", "audit_verdict TEXT")?;
    version::add_column(conn, "tasks", "audit_report_path TEXT")?;
    version::add_column(conn, "tasks", "delivery_assessment TEXT")?;
    version::add_column(conn, "tasks", "dispatch_args TEXT")?;
    version::add_column(conn, "tasks", "final_head_sha TEXT")?;
    version::add_column(conn, "tasks", "final_branch TEXT")?;
    conn.execute_batch(
        "UPDATE tasks
         SET delivery_assessment = verify_status
         WHERE verify_status IN ('empty_diff', 'hollow_output')
           AND delivery_assessment IS NULL;",
    )?;
    conn.execute_batch(
        "UPDATE tasks
         SET verify_status = 'skipped'
         WHERE verify_status IN ('empty_diff', 'hollow_output');",
    )?;
    Ok(())
}

fn migrate_finding_columns(conn: &rusqlite::Connection) -> Result<()> {
    version::add_column(conn, "workgroups", "created_by TEXT")?;
    conn.execute_batch(CREATE_FINDINGS_SQL)?;
    version::add_column(conn, "findings", "severity TEXT")?;
    version::add_column(conn, "findings", "title TEXT")?;
    version::add_column(conn, "findings", "file TEXT")?;
    version::add_column(conn, "findings", "lines TEXT")?;
    version::add_column(conn, "findings", "category TEXT")?;
    version::add_column(conn, "findings", "confidence TEXT")?;
    version::add_column(conn, "findings", "verdict TEXT")?;
    version::add_column(conn, "findings", "score TEXT")?;
    version::add_column(conn, "findings", "note TEXT")?;
    version::add_column(conn, "findings", "updated_at TEXT")?;
    conn.execute_batch(CREATE_KG_SQL)?;
    version::add_column(conn, "tasks", "principal_merge_override TEXT")?;
    Ok(())
}

fn custom_agent_name(
    row: &Row,
    parsed_agent: Option<AgentKind>,
    stored_agent: &str,
) -> Option<String> {
    // An agent name this binary cannot parse is not a custom agent — it is an
    // agent we do not recognise, and the difference is what the reader needs.
    // A task written by a newer aid (t-8e9194dc, agent `commandcode`) used to
    // render as `custom/unknown/unknown` with the real name discarded, so you
    // could not tell which agent had run. Keep the raw string; a genuine
    // custom agent's own name still wins when the column holds one.
    row.get::<_, Option<String>>(26)
        .ok()
        .flatten()
        .or_else(|| parsed_agent.is_none().then(|| stored_agent.to_string()))
}

fn attribution_source(row: &Row) -> Option<AttributionSource> {
    row.get::<_, Option<String>>("attribution_source")
        .ok()
        .flatten()
        .and_then(|value| AttributionSource::parse_str(&value))
}

fn verify_status(row: &Row) -> rusqlite::Result<VerifyStatus> {
    Ok(row
        .get::<_, Option<String>>(27)?
        .and_then(|s| VerifyStatus::parse_str(&s))
        .unwrap_or(VerifyStatus::Skipped))
}

fn delivery_assessment(row: &Row) -> Option<DeliveryAssessment> {
    row.get::<_, Option<String>>(33)
        .ok()
        .flatten()
        .and_then(|value| DeliveryAssessment::parse_str(&value))
}
