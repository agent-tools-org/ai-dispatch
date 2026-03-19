// Workgroup-related store query methods.
// Exports: Store workgroup and finding lookup methods.
// Deps: super::super::Store, rusqlite, crate::types.

use anyhow::Result;
use rusqlite::params;

use super::super::schema::parse_dt;
use super::super::Store;
use crate::types::{Finding, Workgroup, WorkgroupId};

fn row_to_workgroup(row: &rusqlite::Row) -> rusqlite::Result<Result<Workgroup>> {
    Ok(Ok(Workgroup {
        id: WorkgroupId(row.get::<_, String>(0)?),
        name: row.get(1)?,
        shared_context: row.get(2)?,
        created_by: row.get(5).ok().flatten(),
        created_at: parse_dt(&row.get::<_, String>(3)?),
        updated_at: parse_dt(&row.get::<_, String>(4)?),
    }))
}

impl Store {
    pub fn get_workgroup(&self, id: &str) -> Result<Option<Workgroup>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, name, shared_context, created_at, updated_at, created_by
             FROM workgroups WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_workgroup)?;
        match rows.next() {
            Some(row) => Ok(Some(row??)),
            None => Ok(None),
        }
    }

    pub fn list_workgroups(&self) -> Result<Vec<Workgroup>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, name, shared_context, created_at, updated_at, created_by
             FROM workgroups ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_workgroup)?;
        rows.map(|row| row?).collect()
    }

    pub fn list_findings(&self, workgroup_id: &str) -> Result<Vec<Finding>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, workgroup_id, content, source_task_id, severity, title, file, lines, category, confidence, created_at FROM findings
             WHERE workgroup_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![workgroup_id], |row| {
            Ok(Finding {
                id: row.get(0)?,
                workgroup_id: row.get(1)?,
                content: row.get(2)?,
                source_task_id: row.get(3)?,
                severity: row.get(4)?,
                title: row.get(5)?,
                file: row.get(6)?,
                lines: row.get(7)?,
                category: row.get(8)?,
                confidence: row.get(9)?,
                created_at: parse_dt(&row.get::<_, String>(10)?),
            })
        })?;
        rows.map(|row| Ok(row?)).collect()
    }
}
