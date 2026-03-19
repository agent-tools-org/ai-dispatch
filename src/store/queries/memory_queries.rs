// Memory-related store query methods.
// Exports: Store memory list, history, and search methods.
// Deps: super::super::Store, rusqlite, chrono.

use anyhow::Result;
use chrono::Local;
use rusqlite::params;

use super::super::schema::row_to_memory;
use super::super::Store;
use crate::types::{Memory, MemoryType};

fn escape_like(s: &str) -> String {
    s.replace('%', r"\%").replace('_', r"\_")
}

impl Store {
    pub fn list_memories(
        &self,
        project_path: Option<&str>,
        memory_type: Option<MemoryType>,
    ) -> Result<Vec<Memory>> {
        let conn = self.db();
        let now = Local::now().to_rfc3339();
        let type_value = memory_type.map(|memory_type| memory_type.as_str().to_string());
        let mut stmt = conn.prepare(
            "SELECT id, memory_type, content, source_task_id, agent, project_path, content_hash,
             created_at, expires_at, supersedes, version, inject_count, last_injected_at, success_count
             FROM memories
             WHERE (?1 IS NULL OR project_path = ?1)
               AND (?2 IS NULL OR memory_type = ?2)
               AND (expires_at IS NULL OR expires_at > ?3)
               AND id NOT IN (
                   SELECT DISTINCT supersedes FROM memories WHERE supersedes IS NOT NULL
               )
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![project_path, type_value.as_deref(), now], row_to_memory)?;
        rows.map(|row| row?).collect()
    }

    pub fn memory_history(&self, id: &str) -> Result<Vec<Memory>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, memory_type, content, source_task_id, agent, project_path, content_hash,
             created_at, expires_at, supersedes, version, inject_count, last_injected_at, success_count
             FROM memories WHERE id = ?1",
        )?;
        let mut child_stmt = conn.prepare(
            "SELECT id, memory_type, content, source_task_id, agent, project_path, content_hash,
             created_at, expires_at, supersedes, version, inject_count, last_injected_at, success_count
             FROM memories WHERE supersedes = ?1
             ORDER BY version ASC
             LIMIT 1",
        )?;
        let base_memory = match stmt.query_row(params![id], row_to_memory) {
            Ok(row) => row?,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(vec![]),
            Err(err) => return Err(err.into()),
        };
        let mut history = vec![base_memory.clone()];

        let mut previous_id = base_memory.supersedes.as_ref().map(|value| value.as_str().to_string());
        for _ in 0..50 {
            let prev = match previous_id {
                Some(ref prev) => prev.clone(),
                None => break,
            };
            match stmt.query_row(params![prev], row_to_memory) {
                Ok(row) => {
                    let memory = row?;
                    previous_id = memory.supersedes.as_ref().map(|value| value.as_str().to_string());
                    history.push(memory);
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => break,
                Err(err) => return Err(err.into()),
            }
        }

        let mut next_id = Some(base_memory.id.as_str().to_string());
        for _ in 0..50 {
            let curr = match next_id {
                Some(ref curr) => curr.clone(),
                None => break,
            };
            match child_stmt.query_row(params![curr], row_to_memory) {
                Ok(row) => {
                    let memory = row?;
                    next_id = Some(memory.id.as_str().to_string());
                    history.push(memory);
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => break,
                Err(err) => return Err(err.into()),
            }
        }

        history.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(history)
    }

    pub fn search_memories(
        &self,
        query: &str,
        project_path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let conn = self.db();
        let now = Local::now().to_rfc3339();
        let pattern = format!("%{}%", escape_like(query));
        let mut stmt = conn.prepare(
            "SELECT id, memory_type, content, source_task_id, agent, project_path, content_hash,
             created_at, expires_at, supersedes, version, inject_count, last_injected_at, success_count
             FROM memories
             WHERE content LIKE ?1 ESCAPE '\\'
               AND (?2 IS NULL OR project_path = ?2)
               AND (expires_at IS NULL OR expires_at > ?3)
             ORDER BY created_at DESC
             LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![pattern, project_path, now, limit as i64],
            row_to_memory,
        )?;
        rows.map(|row| row?).collect()
    }
}
