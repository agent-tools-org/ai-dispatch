// Conditional worker completion writes and atomic completion events.
// Deps: Store, completion payloads and SQLite transactions.

use anyhow::Result;
use chrono::Local;
use rusqlite::params;

use super::Store;
use crate::types::*;

use super::TaskCompletionUpdate;

impl Store {
    pub fn update_task_completion(&self, payload: TaskCompletionUpdate<'_>) -> Result<bool> {
        self.write_completion(payload, None, true)
    }

    /// Atomically update task completion AND insert the completion event.
    /// Prevents inconsistent state if process crashes between the two writes.
    pub fn complete_task_atomic(
        &self,
        payload: TaskCompletionUpdate<'_>,
        event: &TaskEvent,
    ) -> Result<bool> {
        self.write_completion(payload, Some(event), true)
    }

    pub(crate) fn record_agent_completion(
        &self, payload: TaskCompletionUpdate<'_>, event: Option<&TaskEvent>,
    ) -> Result<bool> {
        self.write_completion(payload, event, false)
    }

    fn write_completion(
        &self, payload: TaskCompletionUpdate<'_>, event: Option<&TaskEvent>, settled: bool,
    ) -> Result<bool> {
        if !self.guard_completion_transition(payload.id, payload.status)? {
            return Ok(false);
        }
        let conn = self.db();
        let tx = conn.unchecked_transaction()?;
        let now = Local::now().to_rfc3339();
        let rows = tx.execute(
            // COALESCE, not assignment: a second completion write that learned
            // nothing must not erase a model the first one observed. The
            // requested model in `model` is never touched here — a completion
            // has no business rewriting what was asked for.
            "UPDATE tasks SET status = ?1, tokens = COALESCE(?2, tokens), duration_ms = ?3,
             completed_at = CASE WHEN ?1 = 'done' AND ?10 = 0 THEN NULL ELSE ?4 END,
             observed_model = COALESCE(?5, observed_model),
             attribution_source = COALESCE(?9, attribution_source),
             cost_usd = COALESCE(?6, cost_usd), exit_code = ?7
             WHERE id = ?8
             AND (status NOT IN ('done', 'failed', 'stopped', 'skipped', 'merged')
                  OR (status = 'done' AND completed_at IS NULL))",
            params![
                payload.status.as_str(),
                payload.tokens,
                payload.duration_ms,
                now,
                payload.observed_model,
                payload.cost_usd,
                payload.exit_code,
                payload.id,
                payload.attribution_source.map(|value| value.as_str()),
                settled,
            ],
        )?;
        if rows == 0 {
            return Ok(false);
        }
        super::usage_checkpoint::reconcile_on(&tx, payload.id, payload.tokens, payload.cost_usd)?;
        if let Some(event) = event {
            super::mutations::insert_event_on(&tx, event)?;
        }
        tx.commit()?;
        Ok(true)
    }
}
