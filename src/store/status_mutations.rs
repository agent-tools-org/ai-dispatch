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
             exit_code = CASE WHEN ?1 = 'failed' AND ?4 = 'done' AND ?6
                 AND (exit_code IS NULL OR exit_code = 0) THEN 1 ELSE exit_code END,
             pending_reason = COALESCE(?5, pending_reason)
             WHERE id = ?2 AND status = ?4
             AND (?1 = 'merged' OR status != 'done' OR completed_at IS NULL)",
            params![status.as_str(), id, Local::now().to_rfc3339(), current.as_str(),
                reason.map(|value| value.as_str()),
                expected.is_some_and(|states| states == [TaskStatus::Done])],
        )?;
        if rows == 0 {
            return Ok(false);
        }
        if status.is_terminal() {
            super::usage_checkpoint::reconcile_on(&tx, id, None, None)?;
        }
        if let Some(event) = event {
            super::mutations::insert_event_on(&tx, event)?;
        }
        tx.commit()?;
        Ok(true)
    }

    pub(crate) fn with_running_task<T>(
        &self,
        id: &str,
        launch: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.with_launch_guard(id, false, launch)
    }

    pub(crate) fn with_worker_launch<T>(
        &self,
        id: &str,
        launch: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.with_launch_guard(id, true, launch)
    }

    fn with_launch_guard<T>(
        &self,
        id: &str,
        check_parent: bool,
        launch: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let conn = self.db();
        let tx = conn.unchecked_transaction()?;
        if tx.execute(
            "UPDATE tasks SET status = status WHERE id = ?1 AND status = 'running'
             AND (?2 = 0 OR CASE WHEN json_valid(dispatch_args)
                 THEN json_extract(dispatch_args, '$.origin') = 'explicit_retry' ELSE 0 END
                 OR NOT EXISTS (
                 SELECT 1 FROM tasks parent WHERE parent.id = tasks.parent_task_id
                 AND parent.status = 'stopped'))",
            params![id, check_parent],
        )? == 0
        {
            if check_parent && let Some(parent) = stop_unlaunched_child(&tx, id)? {
                tx.commit()?;
                anyhow::bail!("Parent task '{parent}' stopped; refusing child '{id}' launch");
            }
            anyhow::bail!("Task '{id}' is no longer running; refusing launch");
        }
        let result = launch()?;
        tx.commit()?;
        Ok(result)
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
        if current == TaskStatus::Done && next == TaskStatus::Stopped {
            return Ok(true);
        }
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
            event_kind: EventKind::Milestone,
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

fn stop_unlaunched_child(tx: &rusqlite::Transaction<'_>, id: &str) -> Result<Option<String>> {
    let parent = tx.query_row(
        "SELECT parent.id FROM tasks child JOIN tasks parent ON parent.id = child.parent_task_id
         WHERE child.id = ?1 AND child.status = 'running' AND parent.status = 'stopped'",
        params![id], |row| row.get::<_, String>(0),
    ).optional()?;
    let Some(parent) = parent else {
        return Ok(None);
    };
    let now = Local::now();
    tx.execute(
        "UPDATE tasks SET status = 'stopped', completed_at = ?2,
         duration_ms = MAX(0, COALESCE(CAST(ROUND((julianday(?2) -
             julianday(COALESCE(started_at, created_at))) * 86400000) AS INTEGER), 0))
         WHERE id = ?1 AND status = 'running'",
        params![id, now.to_rfc3339()],
    )?;
    super::mutations::insert_event_on(
        tx,
        &TaskEvent {
            task_id: TaskId(id.to_string()),
            timestamp: now,
            event_kind: EventKind::Completion,
            detail: format!("Parent task '{parent}' stopped; child launch refused"),
            metadata: None,
        },
    )?;
    Ok(Some(parent))
}
