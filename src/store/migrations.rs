// Store schema migrations for feature-specific tables.
// Exports: migrate_task_messages, migrate_declared_task_profile,
//          migrate_observed_model, migrate_project_id, migrate_effective_dir.
// Deps: anyhow and rusqlite.

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

const CREATE_TASK_MESSAGES_SQL: &str = "CREATE TABLE IF NOT EXISTS task_messages (
    id INTEGER PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    direction TEXT NOT NULL CHECK (direction IN ('in','out')),
    content TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('reply','steer','unstick-auto','agent-ack')),
    created_at DATETIME NOT NULL,
    delivered_at DATETIME,
    acked_at DATETIME
);";

pub(super) fn migrate_task_messages(conn: &Connection) -> Result<()> {
    conn.execute_batch(CREATE_TASK_MESSAGES_SQL)?;
    Ok(())
}

/// Splits model attribution in two. The pre-existing `model` column keeps what
/// aid *requested*; `observed_model` holds what the CLI reported it actually
/// ran, and stays NULL when the CLI said nothing.
///
/// Historical rows are deliberately not rewritten. They were written by a path
/// that fell back to the requested model whenever the CLI stayed silent
/// (`info.model.as_deref().or(model)`), so each is at best a request and at
/// worst a wrong guess: `t-bd455a68` records the `claude` CLI running
/// `gemini-3.6-flash-low` and `t-702f7bcb` records `agy` running cursor's
/// `composer-2`, both of which failed. Reading them as requests is the only
/// honest interpretation left; back-filling `observed_model` from them would
/// launder guesses into observations.
pub(super) fn migrate_observed_model(conn: &Connection) -> Result<()> {
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN observed_model TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN attribution_source TEXT;");
    Ok(())
}

pub(super) fn migrate_declared_task_profile(conn: &Connection) -> Result<()> {
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN declared_difficulty TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN declared_budget TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN declared_urgency TEXT;");
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN declared_rigor TEXT;");
    Ok(())
}

/// First-class project identity on tasks.
///
/// Existing rows are deliberately left NULL. Most historical tasks never
/// recorded a project (or even a repo_path); inventing one would launder
/// guesses into identity. NULL is the explicit unattributed bucket.
pub(super) fn migrate_project_id(conn: &Connection) -> Result<()> {
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN project_id TEXT;");
    let _ = conn
        .execute_batch("CREATE INDEX IF NOT EXISTS idx_tasks_project_id ON tasks(project_id);");
    Ok(())
}

/// Adds `effective_dir` and copies a usable `--dir` out of persisted RunArgs.
///
/// Historical rows predate the column, so a bare ALTER would leave every
/// pre-migration `--dir` task with NULL and make its relative `-o` unresolvable.
/// The directory is already in `dispatch_args.dir`. Only a non-empty absolute
/// path is copied; relative values would reintroduce CWD resolution. Rows with
/// no usable dir stay NULL and report absence.
pub(super) fn migrate_effective_dir(conn: &Connection) -> Result<()> {
    let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN effective_dir TEXT;");
    backfill_effective_dir_from_dispatch_args(conn)
}

fn backfill_effective_dir_from_dispatch_args(conn: &Connection) -> Result<()> {
    let mut select = conn.prepare(
        "SELECT id,
                CASE WHEN json_valid(dispatch_args) THEN
                    CASE WHEN json_type(dispatch_args, '$.dir') = 'text'
                         THEN json_extract(dispatch_args, '$.dir')
                    END
                END
         FROM tasks
         WHERE effective_dir IS NULL",
    )?;
    let rows = select.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let mut updates = Vec::new();
    for row in rows {
        let (id, dir) = row?;
        if let Some(dir) = usable_recorded_dir(dir.as_deref()) {
            updates.push((id, dir));
        }
    }
    drop(select);
    let mut update = conn.prepare(
        "UPDATE tasks SET effective_dir = ?1 WHERE id = ?2 AND effective_dir IS NULL",
    )?;
    for (id, dir) in updates {
        update.execute(rusqlite::params![dir, id])?;
    }
    Ok(())
}

fn usable_recorded_dir(dir: Option<&str>) -> Option<String> {
    let dir = dir.filter(|value| !value.is_empty())?;
    Path::new(dir).is_absolute().then(|| dir.to_string())
}
