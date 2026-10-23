// Bounded event-payload retention and explicit database compaction.
// Extends Store without changing schemas; preserves provider and lifecycle evidence.
use super::super::Store;
use anyhow::Result;
use rusqlite::{Connection, params};
use std::time::Duration;

#[path = "retention_budget.rs"]
mod budget;
#[path = "retention_io.rs"]
mod io;
#[path = "retention_vacuum.rs"]
mod vacuum;
use budget::{Deadline, with_conn};
#[cfg(test)]
use vacuum::require_space;

#[path = "retention_payload.rs"]
mod payload;

const BATCH_ROWS: usize = 200;
const BATCH_BYTES: usize = 512 * 1024;
const TERMINAL: &str = "'done','merged','failed','skipped','stopped'";
const KINDS: &[&str] = &[
    "tool_call",
    "build",
    "test",
    "file_read",
    "file_write",
    "lint",
    "format",
];

#[derive(Default, Debug)]
pub(crate) struct TrimReport {
    pub rows: u64,
    pub bytes: u64,
    pub cursor: i64,
    pub complete: bool,
    pub batches: usize,
    #[cfg(test)]
    pub largest_batch: usize,
}

struct Candidate {
    id: i64,
    kind: String,
    metadata: Option<String>,
    eligible: bool,
    status: String,
    metadata_bytes: usize,
}

impl Store {
    pub(crate) fn trim_event_payloads(
        &self,
        cutoff: &str,
        cursor: i64,
        dry_run: bool,
        budget: Duration,
    ) -> Result<TrimReport> {
        trim(self, cutoff, cursor, dry_run, &Deadline::new(budget))
    }
}

fn candidates(conn: &Connection, cursor: i64, cutoff: &str) -> Result<Vec<Candidate>> {
    let sql = format!(
        "SELECT e.id, e.event_type, CASE WHEN t.status IN ({TERMINAL})
         AND julianday(t.completed_at) < julianday(?2)
         AND octet_length(e.metadata) <= {chunk} THEN e.metadata ELSE NULL END,
         coalesce(t.status IN ({TERMINAL}) AND t.completed_at IS NOT NULL
         AND julianday(t.completed_at) < julianday(?2), 0), t.status,
         coalesce(octet_length(e.metadata), 0)
         FROM events e JOIN tasks t ON t.id=e.task_id
         WHERE e.id > ?1 ORDER BY e.id LIMIT ?3",
        chunk = io::CHUNK_BYTES
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![cursor, cutoff, BATCH_ROWS as i64], |row| {
        Ok(Candidate {
            id: row.get(0)?,
            kind: row.get(1)?,
            metadata: row.get(2)?,
            eligible: row.get(3)?,
            status: row.get(4)?,
            metadata_bytes: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn replacement(row: &Candidate, deadline: &Deadline) -> Result<Option<(String, u64)>> {
    if !row.eligible {
        return Ok(None);
    }
    let Some(raw) = &row.metadata else {
        return Ok(None);
    };
    let Some(value) = deadline.parse(raw) else {
        return Ok(None);
    };
    let mut updated = String::new();
    if deadline.copy(&mut updated, raw).is_none() {
        return Ok(None);
    }
    let mut bytes = 0;
    for key in keys_to_trim(row, &value) {
        let Some(output) = value.get(*key).and_then(|v| v.as_str()) else {
            continue;
        };
        if output.len() < 4096 {
            continue;
        }
        let marker = serde_json::json!({"trimmed": true, "bytes": output.len()});
        if let Some(replaced) =
            payload::replace_string(&updated, key, &marker.to_string(), deadline)
        {
            updated = replaced;
            bytes += output.len() as u64;
        }
    }
    Ok((bytes > 0).then_some((updated, bytes)))
}

fn keys_to_trim(row: &Candidate, value: &serde_json::Value) -> &'static [&'static str] {
    if matches!(row.status.as_str(), "done" | "merged") {
        return &["full", "provider_stdout"];
    }
    if !KINDS.contains(&row.kind.as_str())
        || value.get("exit_code").and_then(|v| v.as_i64()) != Some(0)
        || value.get("command").and_then(|v| v.as_str()).is_none()
        || value.get("status").and_then(|v| v.as_str()) == Some("in_progress")
        || value.get("command").and_then(|v| v.as_str())
            == value.get("full").and_then(|v| v.as_str())
    {
        return &[];
    }
    &["full"]
}

fn trim(
    store: &Store,
    cutoff: &str,
    cursor: i64,
    dry_run: bool,
    deadline: &Deadline,
) -> Result<TrimReport> {
    let mut report = TrimReport {
        cursor,
        ..Default::default()
    };
    while !deadline.expired() {
        let Some(rows) = with_conn(store, deadline, |conn| {
            candidates(conn, report.cursor, cutoff)
        })?
        else {
            break;
        };
        if rows.is_empty() {
            report.complete = true;
            report.cursor = 0;
            break;
        }
        let (updates, cursor) = prepare(store, rows, report.cursor, deadline)?;
        if deadline.expired() {
            break;
        }
        let applied = with_conn(store, deadline, |conn| {
            apply_batch(conn, cutoff, &updates, dry_run, &mut report)
        })?;
        if applied.is_none() {
            break;
        }
        report.cursor = cursor;
    }
    Ok(report)
}

fn prepare(
    store: &Store,
    rows: Vec<Candidate>,
    mut cursor: i64,
    deadline: &Deadline,
) -> Result<(Vec<Update>, i64)> {
    let mut updates = Vec::new();
    let mut batch_bytes = 0;
    for mut row in rows {
        if !io::load(store, &mut row, deadline)? {
            break;
        }
        let replacement = replacement(&row, deadline)?;
        if deadline.expired() {
            break;
        }
        if let Some((metadata, bytes)) = replacement {
            batch_bytes += row.metadata_bytes;
            updates.push((row.id, row.metadata.take(), metadata, bytes, row.status));
        }
        cursor = row.id;
        if batch_bytes >= BATCH_BYTES {
            break;
        }
    }
    Ok((updates, cursor))
}

type Update = (i64, Option<String>, String, u64, String);

fn apply_batch(
    conn: &Connection,
    cutoff: &str,
    updates: &[Update],
    dry_run: bool,
    report: &mut TrimReport,
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    let mut changed_rows = 0;
    let mut changed_bytes = 0;
    for (id, original, marker, bytes, status) in updates {
        let changed = if dry_run {
            1
        } else {
            tx.execute(
                &format!(
                    "UPDATE events SET metadata=?1
                 WHERE id=?2 AND metadata=?3 AND EXISTS
                 (SELECT 1 FROM tasks WHERE tasks.id=events.task_id
                  AND status=?5 AND status IN ({TERMINAL})
                  AND julianday(completed_at) < julianday(?4))"
                ),
                params![marker, id, original, cutoff, status],
            )?
        };
        changed_rows += changed as u64;
        changed_bytes += bytes * changed as u64;
    }
    tx.commit()?;
    report.rows += changed_rows;
    report.bytes += changed_bytes;
    report.batches += 1;
    #[cfg(test)]
    {
        report.largest_batch = report.largest_batch.max(updates.len());
    }
    Ok(())
}

#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "retention_status_tests.rs"]
mod status_tests;

#[cfg(test)]
#[path = "retention_budget_tests.rs"]
mod budget_tests;
