// Indexed API-route launch evidence and resumable bounded historical ingestion.
// Uses SQLite triggers for atomic event writes and short cursor transactions for backfill.
use anyhow::Result;
use rusqlite::{Connection, TransactionBehavior, params};
use std::time::{Duration, Instant};

const BATCH_SIZE: i64 = 2_000;
const BUDGET: Duration = Duration::from_millis(25);
pub(super) const LOOKUP: &str = "SELECT task_id FROM api_route_launches WHERE route = ?1";
pub(super) const LEGACY: &str = "SELECT DISTINCT task_id FROM events
    WHERE metadata LIKE '%api_route_launch%'
      AND json_extract(CASE WHEN json_valid(metadata) THEN metadata END,
                       '$.api_route_launch') = ?1";

pub(super) fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS api_route_launches (
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            route TEXT NOT NULL, PRIMARY KEY(task_id, route)
         );
         CREATE INDEX IF NOT EXISTS idx_api_route_launches_route ON api_route_launches(route, task_id);
         CREATE TABLE IF NOT EXISTS api_route_launch_backfill (
            singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
            cursor INTEGER, complete INTEGER NOT NULL DEFAULT 0
         );
         INSERT OR IGNORE INTO api_route_launch_backfill(singleton) VALUES (1);
         CREATE TRIGGER IF NOT EXISTS record_api_route_launch AFTER INSERT ON events
         WHEN NEW.metadata LIKE '%api_route_launch%'
         BEGIN
            INSERT OR IGNORE INTO api_route_launches(task_id, route)
            SELECT NEW.task_id, json_extract(
                CASE WHEN json_valid(NEW.metadata) THEN NEW.metadata END, '$.api_route_launch')
            WHERE typeof(json_extract(
                CASE WHEN json_valid(NEW.metadata) THEN NEW.metadata END,
                '$.api_route_launch')) = 'text';
         END;",
    )?;
    Ok(())
}

fn complete(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT complete FROM api_route_launch_backfill WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?)
}

pub(super) fn backfill_batch(conn: &mut Connection, limit: i64) -> Result<bool> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let cursor: Option<i64> = tx.query_row(
        "SELECT cursor FROM api_route_launch_backfill WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    let sql = if cursor.is_some() {
        "SELECT id FROM events WHERE id > ?1 ORDER BY id LIMIT ?2"
    } else {
        "SELECT id FROM events WHERE ?1 IS NULL ORDER BY id LIMIT ?2"
    };
    let ids = tx
        .prepare(sql)?
        .query_map(params![cursor, limit], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if let (Some(first), Some(last)) = (ids.first(), ids.last()) {
        tx.execute(
            "INSERT OR IGNORE INTO api_route_launches(task_id, route)
             SELECT task_id, json_extract(metadata, '$.api_route_launch') FROM events
             WHERE id BETWEEN ?1 AND ?2 AND metadata LIKE '%api_route_launch%'
               AND typeof(json_extract(CASE WHEN json_valid(metadata) THEN metadata END,
                                       '$.api_route_launch')) = 'text'",
            params![first, last],
        )?;
        tx.execute(
            "UPDATE api_route_launch_backfill SET cursor = ?1 WHERE singleton = 1",
            [last],
        )?;
    }
    let done = ids.len() < limit as usize;
    tx.execute(
        "UPDATE api_route_launch_backfill SET complete = ?1 WHERE singleton = 1",
        [done],
    )?;
    tx.commit()?;
    Ok(done)
}

pub(super) fn lookup(conn: &mut Connection, route: &str) -> Result<Vec<String>> {
    lookup_with_budget(conn, route, BUDGET)
}

fn lookup_with_budget(conn: &mut Connection, route: &str, budget: Duration) -> Result<Vec<String>> {
    let start = Instant::now();
    let mut done = complete(conn)?;
    while !done && start.elapsed() < budget {
        done = backfill_batch(conn, BATCH_SIZE)?;
    }
    let mut stmt = conn.prepare(if done { LOOKUP } else { LEGACY })?;
    let rows = stmt.query_map([route], |row| row.get(0))?;
    rows.map(|row| Ok(row?)).collect()
}

#[cfg(test)]
#[path = "route_launch_tests.rs"]
mod tests;
