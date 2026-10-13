// Conditional task status settlement and launch serialization.
// Deps: Store, SQLite transactions, task status guards and event persistence.

use anyhow::Result;
use chrono::Local;
use rusqlite::{params, OptionalExtension};

use super::Store;
use crate::types::*;

impl Store {
    pub fn update_task_status(&self, id: &str, status: TaskStatus) -> Result<bool> {
        self.settle_task_status(id, status, None, None, None)
    }

    pub(crate) fn settle_task_status(
        &self, id: &str, status: TaskStatus, expected: Option<&[TaskStatus]>,
        reason: Option<PendingReason>, event: Option<&TaskEvent>,
    ) -> Result<bool> {
        let Some(current) = self.current_task_status(id)? else { return Ok(false) };
        if expected.is_some_and(|states| !states.contains(&current))
            || !self.guard_known_status_transition(id, current, status)?
        {
            return Ok(false);
        }
        let conn = self.db();
        let tx = conn.unchecked_transaction()?;
        let rows = tx.execute(
            "UPDATE tasks SET status = ?1,
             started_at = CASE WHEN ?1 = 'running' THEN COALESCE(started_at, ?3) ELSE started_at END,
             completed_at = CASE WHEN ?1 IN ('done', 'failed', 'stopped') THEN ?3 ELSE completed_at END,
             duration_ms = CASE WHEN ?1 IN ('failed', 'stopped')
                 OR (?1 = 'done' AND duration_ms IS NULL) THEN
                 MAX(0, COALESCE(CAST(ROUND((julianday(?3) -
                     julianday(COALESCE(started_at, created_at))) * 86400000) AS INTEGER), 0))
                 ELSE duration_ms END,
             exit_code = CASE WHEN ?1 = 'failed' AND ?4 = 'done'
                 AND (exit_code IS NULL OR exit_code = 0) THEN 1 ELSE exit_code END,
             pending_reason = COALESCE(?5, pending_reason)
             WHERE id = ?2 AND status = ?4
             AND (?1 = 'merged' OR status != 'done' OR completed_at IS NULL)",
            params![status.as_str(), id, Local::now().to_rfc3339(), current.as_str(),
                reason.map(|value| value.as_str())],
        )?;
        if rows == 0 {
            return Ok(false);
        }
        if let Some(event) = event {
            super::mutations::insert_event_on(&tx, event)?;
        }
        tx.commit()?;
        Ok(true)
    }

    pub(crate) fn with_running_task<T>(&self, id: &str, launch: impl FnOnce() -> Result<T>) -> Result<T> {
        let conn = self.db();
        let tx = conn.unchecked_transaction()?;
        if tx.execute("UPDATE tasks SET status = status WHERE id = ?1 AND status = 'running'",
            params![id])? == 0
        {
            anyhow::bail!("Task '{id}' is no longer running; refusing launch");
        }
        let result = launch()?;
        tx.commit()?;
        Ok(result)
    }

    /// Set status to Failed only if currently Running or Waiting.
    /// Prevents zombie cleanup from clobbering a real completion status.
    pub fn fail_if_running(&self, id: &str) -> Result<bool> {
        self.settle_task_status(id, TaskStatus::Failed,
            Some(&[TaskStatus::Running, TaskStatus::Waiting]), None, None)
    }

    pub fn fail_pending_with_reason(
        &self,
        id: &str,
        pending_reason: PendingReason,
    ) -> Result<bool> {
        self.settle_task_status(id, TaskStatus::Failed, Some(&[TaskStatus::Pending]),
            Some(pending_reason), None)
    }

    pub fn fail_waiting_with_reason(&self, id: &str, detail: &str) -> Result<bool> {
        let event = TaskEvent {
            task_id: TaskId(id.to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: detail.to_string(),
            metadata: Some(serde_json::json!({ "failure_kind": "wait_timeout" })),
        };
        self.settle_task_status(id, TaskStatus::Failed, Some(&[TaskStatus::Waiting]),
            Some(PendingReason::WaitTimeout), Some(&event))
    }

    pub(super) fn guard_completion_transition(&self, id: &str, next: TaskStatus) -> Result<bool> {
        let Some(current) = self.current_task_status(id)? else {
            return Ok(false);
        };
        if matches!(current,
            TaskStatus::Failed | TaskStatus::Stopped | TaskStatus::Skipped | TaskStatus::Merged)
        {
            return Ok(false);
        }
        self.guard_known_status_transition(id, current, next)
    }

    pub(crate) fn guard_current_status(
        &self,
        id: &str,
        expected: &[TaskStatus],
        next: TaskStatus,
    ) -> Result<bool> {
        let Some(current) = self.current_task_status(id)? else {
            return Ok(false);
        };
        if !expected.contains(&current) {
            return Ok(false);
        }
        self.guard_known_status_transition(id, current, next)
    }

    fn guard_known_status_transition(
        &self,
        id: &str,
        current: TaskStatus,
        next: TaskStatus,
    ) -> Result<bool> {
        if current.is_terminal() && next.is_terminal() && next != TaskStatus::Merged
            && !(current == TaskStatus::Done && self.provisional_done(id)?)
        {
            return Ok(false);
        }
        if current == TaskStatus::Done && next == TaskStatus::Stopped { return Ok(true); }
        if current.can_transition_to(next) {
            return Ok(true);
        }
        aid_warn!(
            "[aid] Rejected illegal status transition for {id}: {} -> {}",
            current.as_str(),
            next.as_str()
        );
        self.record_rejected_transition(id, current, next)?;
        Ok(false)
    }

    /// Persist a rejected transition on the task's event log so `aid show` records it.
    fn record_rejected_transition(
        &self,
        id: &str,
        current: TaskStatus,
        next: TaskStatus,
    ) -> Result<()> {
        self.insert_event(&TaskEvent {
            task_id: TaskId(id.to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: format!(
                "rejected illegal status transition: {} -> {}",
                current.as_str(),
                next.as_str()
            ),
            metadata: Some(serde_json::json!({
                "from": current.as_str(),
                "to": next.as_str(),
            })),
        })
    }

    fn provisional_done(&self, id: &str) -> Result<bool> {
        Ok(self.db().query_row(
            "SELECT status = 'done' AND completed_at IS NULL FROM tasks WHERE id = ?1",
            params![id], |row| row.get::<_, bool>(0),
        ).optional()?.unwrap_or(false))
    }

    fn current_task_status(&self, id: &str) -> Result<Option<TaskStatus>> {
        let status = self
            .db()
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(status.and_then(|value| TaskStatus::parse_str(&value)))
    }
}
