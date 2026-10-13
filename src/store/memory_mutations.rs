// Agent memory insertion, editing and usage counters.
// Deps: Store, memory row decoding and SQLite parameters.

use anyhow::Result;
use chrono::Local;
use rusqlite::params;

use super::schema::row_to_memory;
use super::Store;
use crate::types::*;

impl Store {
    pub fn insert_memory(&self, memory: &Memory) -> Result<()> {
        self.db().execute(
            "INSERT OR IGNORE INTO memories (id, memory_type, content, source_task_id, agent,
              project_path, content_hash, created_at, expires_at, supersedes, version,
              inject_count, last_injected_at, success_count, tier)
              VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                memory.id.as_str(),
                memory.memory_type.as_str(),
                memory.content,
                memory.source_task_id,
                memory.agent,
                memory.project_path,
                memory.content_hash,
                memory.created_at.to_rfc3339(),
                memory.expires_at.map(|dt| dt.to_rfc3339()),
                memory.supersedes.as_ref().map(|id| id.as_str()),
                memory.version,
                memory.inject_count,
                memory.last_injected_at.map(|dt| dt.to_rfc3339()),
                memory.success_count,
                memory.tier.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn update_memory(&self, id: &str, content: &str) -> Result<bool> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(content, &mut hasher);
        let hash = format!("{:016x}", std::hash::Hasher::finish(&hasher));
        let now = chrono::Local::now().to_rfc3339();
        let conn = self.db();
        let existing = match conn.query_row(
            "SELECT id, memory_type, content, source_task_id, agent, project_path, content_hash,
             created_at, expires_at, supersedes, version, inject_count, last_injected_at, success_count, tier
             FROM memories WHERE id = ?1",
            params![id],
            row_to_memory,
        ) {
            Ok(row) => row?,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(false),
            Err(err) => return Err(err.into()),
        };
        let Memory {
            id: old_id,
            memory_type,
            tier,
            source_task_id,
            agent,
            project_path,
            expires_at,
            version,
            inject_count,
            last_injected_at,
            success_count,
            ..
        } = existing;
        let supersedes_id = old_id.as_str().to_string();
        let expires_at_str = expires_at.map(|dt| dt.to_rfc3339());
        let last_injected_at_str = last_injected_at.map(|dt| dt.to_rfc3339());
        let new_id = MemoryId::generate();
        conn.execute(
            "INSERT INTO memories (id, memory_type, content, source_task_id, agent, project_path,
             content_hash, created_at, expires_at, supersedes, version, inject_count, last_injected_at,
             success_count, tier)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                new_id.as_str(),
                memory_type.as_str(),
                content,
                source_task_id,
                agent,
                project_path,
                hash,
                now,
                expires_at_str,
                supersedes_id,
                version + 1,
                inject_count,
                last_injected_at_str,
                success_count,
                tier.as_str(),
            ],
        )?;
        Ok(true)
    }

    pub fn increment_memory_inject(&self, id: &str) -> Result<bool> {
        let now = Local::now().to_rfc3339();
        let rows = self.db().execute(
            "UPDATE memories SET inject_count = inject_count + 1, last_injected_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(rows > 0)
    }

    pub fn increment_memory_success(&self, id: &str) -> Result<bool> {
        let rows = self.db().execute(
            "UPDATE memories SET success_count = success_count + 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(rows > 0)
    }

    pub fn delete_memory(&self, id: &str) -> Result<()> {
        self.db()
            .execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }
}
