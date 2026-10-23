// Indexed API-route launch evidence and resumable bounded historical ingestion.
// Uses SQLite triggers for atomic event writes and short cursor transactions for backfill.
use super::Store;
use anyhow::Result;
use rusqlite::{Connection, TransactionBehavior, params};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
#[path = "route_launch_read.rs"]
mod read;

const BATCH_SIZE: i64 = 64;
const BUDGET: Duration = Duration::from_millis(25);
pub(super) const LOOKUP: &str = "SELECT DISTINCT task_id FROM api_route_launches WHERE route = ?1";
#[cfg(test)]
pub(super) const LEGACY: &str = "SELECT DISTINCT task_id FROM events
    WHERE metadata LIKE '%api_route_launch%'
      AND json_extract(CASE WHEN json_valid(metadata) THEN metadata END,
                       '$.api_route_launch') = ?1";

pub(super) fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS api_route_launches (
            event_id INTEGER PRIMARY KEY REFERENCES events(id) ON DELETE CASCADE,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            route TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_api_route_launches_route ON api_route_launches(route, task_id);
         CREATE TABLE IF NOT EXISTS api_route_launch_backfill (
            singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
            cursor INTEGER, complete INTEGER NOT NULL DEFAULT 0
         );
         INSERT OR IGNORE INTO api_route_launch_backfill(singleton) VALUES (1);",
    )?;
    create_triggers(conn)
}

fn create_triggers(conn: &Connection) -> Result<()> {
    for (name, action, remove) in [
        ("record_api_route_launch", "INSERT", ""),
        (
            "update_api_route_launch",
            "UPDATE OF id, task_id, metadata",
            "DELETE FROM api_route_launches WHERE event_id = OLD.id;",
        ),
    ] {
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS {name} AFTER {action} ON events BEGIN
                {remove}
                INSERT OR REPLACE INTO api_route_launches(event_id, task_id, route)
                SELECT NEW.id, NEW.task_id, json_extract(
                    CASE WHEN json_valid(NEW.metadata) THEN NEW.metadata END, '$.api_route_launch')
                WHERE NEW.metadata LIKE '%api_route_launch%'
                  AND typeof(json_extract(CASE WHEN json_valid(NEW.metadata) THEN NEW.metadata END,
                                          '$.api_route_launch')) = 'text';
             END;"
        ))?;
    }
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS delete_api_route_launch AFTER DELETE ON events BEGIN
            DELETE FROM api_route_launches WHERE event_id = OLD.id;
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

pub(super) fn backfill_batch(store: &Store, limit: i64) -> Result<bool> {
    let (cursor, expected) = {
        let conn = store.db();
        if complete(&conn)? {
            return Ok(true);
        }
        (cursor(&conn)?, read::stamp(&conn)?)
    };
    let Some(batch) = read::scan(store, cursor, limit, expected)? else {
        return Ok(false);
    };
    let conn = store.db();
    let timeout: u64 = conn.query_row("PRAGMA busy_timeout", [], |row| row.get(0))?;
    conn.busy_timeout(Duration::ZERO)?;
    let result = persist_batch(&conn, batch, expected);
    conn.busy_timeout(Duration::from_millis(timeout))?;
    result
}

fn persist_batch(conn: &Connection, batch: read::Batch, expected: read::Stamp) -> Result<bool> {
    let tx = match rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate) {
        Ok(tx) => tx,
        Err(rusqlite::Error::SqliteFailure(error, _))
            if matches!(
                error.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    if read::stamp(&tx)? != expected {
        return Ok(false);
    }
    for (event, task, route) in batch.launches {
        tx.execute(
            "INSERT OR IGNORE INTO api_route_launches(event_id, task_id, route) VALUES (?1, ?2, ?3)",
            params![event, task, route],
        )?;
    }
    tx.execute(
        "UPDATE api_route_launch_backfill SET cursor = ?1, complete = ?2 WHERE singleton = 1",
        params![batch.cursor, batch.done],
    )?;
    tx.commit()?;
    Ok(batch.done)
}

fn cursor(conn: &Connection) -> Result<Option<i64>> {
    Ok(conn.query_row(
        "SELECT cursor FROM api_route_launch_backfill WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?)
}

pub(super) fn lookup(store: &Store, route: &str) -> Result<Vec<String>> {
    lookup_with_budget(store, route, BUDGET)
}

fn lookup_with_budget(store: &Store, route: &str, budget: Duration) -> Result<Vec<String>> {
    let start = Instant::now();
    while start.elapsed() < budget && !complete(&store.db())? {
        if backfill_batch(store, BATCH_SIZE)? {
            break;
        }
    }
    loop {
        if let Some(results) = pending_lookup(store, route)? {
            return Ok(results);
        }
    }
}

fn pending_lookup(store: &Store, route: &str) -> Result<Option<Vec<String>>> {
    let (mut cursor, expected, done, mut tasks) = {
        let conn = store.db();
        let expected = read::stamp(&conn)?;
        let tasks = conn
            .prepare(LOOKUP)?
            .query_map([route], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<BTreeSet<_>>>()?;
        let cursor = cursor(&conn)?;
        let done = complete(&conn)?;
        if read::stamp(&conn)? != expected {
            return Ok(None);
        }
        (cursor, expected, done, tasks)
    };
    if done {
        return Ok(Some(tasks.into_iter().collect()));
    }
    loop {
        let Some(batch) = read::scan(store, cursor, BATCH_SIZE, expected)? else {
            return Ok(None);
        };
        for (_, task, launch_route) in batch.launches {
            if launch_route == route {
                tasks.insert(task);
            }
        }
        cursor = batch.cursor;
        if batch.done {
            return Ok((read::stamp(&store.db())? == expected).then(|| tasks.into_iter().collect()));
        }
    }
}

#[cfg(test)]
#[path = "route_launch_tests.rs"]
mod tests;
