// Secondary SQLite row mappers kept separate from the core task schema.
// Exports: row_to_event(), row_to_memory().
// Deps: schema::parse_dt, rusqlite rows, event and memory domain types.

use anyhow::Result;
use rusqlite::Row;

use super::schema::parse_dt;
use crate::types::{EventKind, Memory, MemoryId, MemoryTier, MemoryType, TaskEvent, TaskId};

pub(super) fn row_to_event(row: &Row) -> rusqlite::Result<TaskEvent> {
    let metadata_str: Option<String> = row.get(4)?;
    let metadata = metadata_str.and_then(|value| serde_json::from_str(&value).ok());
    Ok(TaskEvent {
        task_id: TaskId(row.get::<_, String>(0)?),
        timestamp: parse_dt(&row.get::<_, String>(1)?),
        event_kind: EventKind::parse_or_warn(&row.get::<_, String>(2)?),
        detail: row.get(3)?,
        metadata,
    })
}

pub(super) fn row_to_memory(row: &Row) -> rusqlite::Result<Result<Memory>> {
    Ok(Ok(Memory {
        id: MemoryId(row.get::<_, String>(0)?),
        memory_type: MemoryType::parse_str(&row.get::<_, String>(1)?).unwrap_or(MemoryType::Fact),
        tier: row.get::<_, Option<String>>(14)?
            .and_then(|value| MemoryTier::parse_str(&value))
            .unwrap_or(MemoryTier::OnDemand),
        content: row.get(2)?, source_task_id: row.get(3)?, agent: row.get(4)?,
        project_path: row.get(5)?, content_hash: row.get(6)?,
        created_at: parse_dt(&row.get::<_, String>(7)?),
        expires_at: row.get::<_, Option<String>>(8)?.map(|value| parse_dt(&value)),
        supersedes: row.get::<_, Option<String>>(9)?.map(MemoryId),
        version: row.get::<_, i64>(10)?, inject_count: row.get::<_, i64>(11)?,
        last_injected_at: row.get::<_, Option<String>>(12)?.map(|value| parse_dt(&value)),
        success_count: row.get::<_, i64>(13)?,
    }))
}
