// Shared completion payloads, workgroup writes and event persistence.
// Exports: Store mutation methods.
// Deps: rusqlite, chrono, crate::types.

use anyhow::Result;
use chrono::Local;
use rusqlite::params;

use super::Store;
use crate::types::*;

pub struct TaskCompletionUpdate<'a> {
    pub id: &'a str,
    pub status: TaskStatus,
    pub tokens: Option<i64>,
    pub duration_ms: i64,
    /// What the CLI reported it actually ran, or `None` when it reported
    /// nothing. Never the dispatched request — writing that here is what made
    /// `t-bd455a68` claim the `claude` CLI ran `gemini-3.6-flash-low`.
    pub observed_model: Option<&'a str>,
    /// The grade of `observed_model`. Moves with it: `None` whenever the model
    /// is `None`.
    pub attribution_source: Option<AttributionSource>,
    pub cost_usd: Option<f64>,
    pub exit_code: Option<i32>,
}

impl Store {
    pub fn create_workgroup(
        &self,
        name: &str,
        shared_context: &str,
        created_by: Option<&str>,
        custom_id: Option<&str>,
    ) -> Result<Workgroup> {
        let now = Local::now();
        let workgroup = Workgroup {
            id: custom_id
                .map(|s| WorkgroupId(s.to_string()))
                .unwrap_or_else(WorkgroupId::generate),
            name: name.to_string(),
            shared_context: shared_context.to_string(),
            created_by: created_by.map(str::to_string),
            created_at: now,
            updated_at: now,
        };
        self.db().execute(
            "INSERT INTO workgroups (id, name, shared_context, created_by, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                workgroup.id.as_str(),
                workgroup.name,
                workgroup.shared_context,
                workgroup.created_by,
                workgroup.created_at.to_rfc3339(),
                workgroup.updated_at.to_rfc3339(),
            ],
        )?;
        let workspace_dir = crate::paths::workspace_dir(workgroup.id.as_str())?;
        if let Err(e) = std::fs::create_dir_all(&workspace_dir) {
            // Rollback DB row to avoid orphaned workgroup without workspace
            let _ = self.db().execute(
                "DELETE FROM workgroups WHERE id = ?1",
                params![workgroup.id.as_str()],
            );
            return Err(e.into());
        }
        Ok(workgroup)
    }

    pub fn insert_event(&self, event: &TaskEvent) -> Result<()> {
        insert_event_on(&self.db(), event)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_finding(
        &self,
        workgroup_id: &str,
        content: &str,
        source_task_id: Option<&str>,
        severity: Option<&str>,
        title: Option<&str>,
        file: Option<&str>,
        lines: Option<&str>,
        category: Option<&str>,
        confidence: Option<&str>,
    ) -> Result<()> {
        let now = Local::now().to_rfc3339();
        self.db().execute(
            "INSERT INTO findings (workgroup_id, content, source_task_id, severity, title, file, lines, category, confidence, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                workgroup_id,
                content,
                source_task_id,
                severity,
                title,
                file,
                lines,
                category,
                confidence,
                now,
            ],
        )?;
        Ok(())
    }
}

pub(super) fn insert_event_on(conn: &rusqlite::Connection, event: &TaskEvent) -> Result<()> {
    let metadata_str = event.metadata.as_ref().map(|m| m.to_string());
    conn.execute(
        "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            event.task_id.as_str(),
            event.timestamp.to_rfc3339(),
            event.event_kind.as_str(),
            event.detail,
            metadata_str,
        ],
    )?;
    Ok(())
}
