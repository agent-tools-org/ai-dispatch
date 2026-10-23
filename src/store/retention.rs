// Bounded event-payload retention and explicit database compaction.
// Extends Store without changing schemas; preserves provider and lifecycle evidence.
use super::super::Store;
use anyhow::{Result, bail};
use rusqlite::{Connection, params};
use std::time::{Duration, Instant};

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
}

impl Store {
    pub(crate) fn trim_event_payloads(
        &self,
        cutoff: &str,
        cursor: i64,
        dry_run: bool,
        budget: Duration,
    ) -> Result<TrimReport> {
        let conn = self.db();
        let timeout: u64 = conn.query_row("PRAGMA busy_timeout", [], |row| row.get(0))?;
        conn.busy_timeout(Duration::from_millis(25))?;
        let result = trim(&conn, cutoff, cursor, dry_run, budget);
        let restored = conn.busy_timeout(Duration::from_millis(timeout));
        restored?;
        result
    }

    pub(crate) fn vacuum_events(&self) -> Result<(u64, u64)> {
        let conn = self.db();
        let path: String = conn.query_row("PRAGMA database_list", [], |row| row.get(2))?;
        let before = std::fs::metadata(&path)?.len();
        let available = crate::support::system_resources::available_disk_mb(&path)
            .ok_or_else(|| anyhow::anyhow!("Cannot determine free disk space for VACUUM"))?;
        require_space(before, available)?;
        conn.execute_batch("PRAGMA locking_mode=EXCLUSIVE;")?;
        let result = conn
            .execute_batch("BEGIN EXCLUSIVE;")
            .map_err(anyhow::Error::from)
            .and_then(|()| vacuum_idle(&conn));
        if !conn.is_autocommit() {
            conn.execute_batch("ROLLBACK;")?;
        }
        conn.execute_batch(
            "PRAGMA locking_mode=NORMAL; BEGIN; SELECT count(*) FROM sqlite_schema; COMMIT;",
        )?;
        result?;
        Ok((before, std::fs::metadata(path)?.len()))
    }
}

fn require_space(size: u64, available_mb: u64) -> Result<()> {
    let required = size.saturating_mul(2).div_ceil(1024 * 1024);
    if available_mb < required {
        bail!(
            "VACUUM needs temporary free disk space up to twice the database size: \
               {required} MB required, {available_mb} MB available"
        );
    }
    Ok(())
}

fn vacuum_idle(conn: &Connection) -> Result<()> {
    let active: i64 = conn.query_row(
        &format!("SELECT count(*) FROM tasks WHERE status NOT IN ({TERMINAL})"),
        [],
        |row| row.get(0),
    )?;
    if active > 0 {
        bail!("VACUUM refused: {active} tasks are running, waiting, or otherwise active");
    }
    conn.execute_batch("COMMIT; VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

fn candidates(conn: &Connection, cursor: i64, cutoff: &str) -> Result<Vec<Candidate>> {
    let sql = format!(
        "SELECT e.id, e.event_type, CASE WHEN t.status IN ({TERMINAL})
         AND julianday(t.completed_at) < julianday(?2) THEN e.metadata ELSE NULL END,
         coalesce(t.status IN ({TERMINAL}) AND t.completed_at IS NOT NULL
         AND julianday(t.completed_at) < julianday(?2), 0)
         FROM events e JOIN tasks t ON t.id=e.task_id
         WHERE e.id > ?1 ORDER BY e.id LIMIT ?3"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![cursor, cutoff, BATCH_ROWS as i64], |row| {
        Ok(Candidate {
            id: row.get(0)?,
            kind: row.get(1)?,
            metadata: row.get(2)?,
            eligible: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn replacement(row: &Candidate) -> Result<Option<(String, u64)>> {
    if !row.eligible || !KINDS.contains(&row.kind.as_str()) {
        return Ok(None);
    }
    let Some(raw) = &row.metadata else {
        return Ok(None);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Ok(None);
    };
    if value.get("exit_code").and_then(|v| v.as_i64()) != Some(0)
        || value.get("command").and_then(|v| v.as_str()).is_none()
        || value.get("status").and_then(|v| v.as_str()) == Some("in_progress")
    {
        return Ok(None);
    }
    let Some(full) = value.get("full").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    if full.len() < 4096 || value.get("command").and_then(|v| v.as_str()) == Some(full) {
        return Ok(None);
    }
    let marker = serde_json::json!({"trimmed": true, "bytes": full.len()});
    Ok(payload::replace_full(raw, &marker.to_string()).map(|updated| (updated, full.len() as u64)))
}

fn trim(
    conn: &Connection,
    cutoff: &str,
    cursor: i64,
    dry_run: bool,
    budget: Duration,
) -> Result<TrimReport> {
    let started = Instant::now();
    let mut report = TrimReport {
        cursor,
        ..Default::default()
    };
    while started.elapsed() < budget {
        let rows = candidates(conn, report.cursor, cutoff)?;
        if rows.is_empty() {
            report.complete = true;
            report.cursor = 0;
            break;
        }
        let mut updates = Vec::new();
        let mut batch_bytes = 0;
        for row in rows {
            if started.elapsed() >= budget {
                break;
            }
            if let Some((marker, bytes)) = replacement(&row)? {
                batch_bytes += row.metadata.as_ref().map_or(0, String::len);
                updates.push((row.id, row.metadata.clone(), marker, bytes));
            }
            report.cursor = row.id;
            if batch_bytes >= BATCH_BYTES {
                break;
            }
        }
        apply_batch(conn, cutoff, &updates, dry_run, &mut report)?;
    }
    Ok(report)
}

type Update = (i64, Option<String>, String, u64);

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
    for (id, original, marker, bytes) in updates {
        let changed = if dry_run {
            1
        } else {
            tx.execute(
                &format!(
                    "UPDATE events SET metadata=?1
                 WHERE id=?2 AND metadata=?3 AND EXISTS
                 (SELECT 1 FROM tasks WHERE tasks.id=events.task_id AND status IN ({TERMINAL})
                  AND julianday(completed_at) < julianday(?4))"
                ),
                params![marker, id, original, cutoff],
            )?
        };
        report.rows += changed as u64;
        report.bytes += bytes * changed as u64;
    }
    tx.commit()?;
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
