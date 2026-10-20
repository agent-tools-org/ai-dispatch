// Durable normalized usage snapshots, separate from terminal lifecycle state.
// Atomically checkpoints accepted agent events and reconciles settlement accounting.
use anyhow::Result;
use rusqlite::{Connection, params};

use super::Store;
use crate::types::{CompletionInfo, TaskEvent};

pub(super) fn create_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_usage_checkpoints (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             tokens INTEGER,
             cost_usd REAL
         );",
    )?;
    Ok(())
}

impl Store {
    pub(crate) fn insert_agent_event_with_usage(
        &self,
        event: &TaskEvent,
        info: &CompletionInfo,
    ) -> Result<()> {
        let conn = self.db();
        let tx = conn.unchecked_transaction()?;
        let rows = super::mutations::insert_event_with_guard(&tx, event, true)?;
        if rows > 0 && (info.tokens.is_some() || info.cost_usd.is_some()) {
            tx.execute(
                "INSERT INTO task_usage_checkpoints (task_id, tokens, cost_usd)
                 VALUES (?1, ?2, ?3) ON CONFLICT(task_id) DO UPDATE SET
                 tokens = COALESCE(excluded.tokens, tokens),
                 cost_usd = COALESCE(excluded.cost_usd, cost_usd)",
                params![event.task_id.as_str(), info.tokens, info.cost_usd],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

pub(super) fn reconcile_on(
    conn: &Connection,
    id: &str,
    tokens: Option<i64>,
    cost_usd: Option<f64>,
) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET
         tokens = COALESCE(?2, tokens, (SELECT tokens FROM task_usage_checkpoints WHERE task_id = ?1)),
         cost_usd = COALESCE(?3, cost_usd, (SELECT cost_usd FROM task_usage_checkpoints WHERE task_id = ?1))
         WHERE id = ?1",
        params![id, tokens, cost_usd],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "usage_checkpoint_tests.rs"]
mod tests;
