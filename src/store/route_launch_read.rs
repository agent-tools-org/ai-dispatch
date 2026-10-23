// Bounded metadata reads for launch backfill and the not-yet-projected lookup tail.
// Releases Store locks between byte slices and decodes JSON on a private SQLite connection.
use super::*;
use rusqlite::OptionalExtension;

pub(super) const BYTE_LIMIT: usize = 256 * 1024;
pub(super) type Stamp = (u64, i64);
pub(super) type Launch = (i64, String, String);

pub(super) struct Range {
    pub rows: Vec<(i64, String, usize, bool)>,
    pub done: bool,
}

pub(super) struct Batch {
    pub cursor: Option<i64>,
    pub done: bool,
    pub launches: Vec<Launch>,
}

pub(super) fn stamp(conn: &Connection) -> Result<Stamp> {
    Ok((
        conn.total_changes(),
        conn.query_row("PRAGMA data_version", [], |r| r.get(0))?,
    ))
}

pub(super) fn range(conn: &Connection, cursor: Option<i64>, limit: i64) -> Result<Range> {
    let sql = if cursor.is_some() {
        "SELECT id, task_id, coalesce(octet_length(metadata), 0), typeof(metadata) = 'blob'
         FROM events WHERE id > ?1 ORDER BY id LIMIT ?2"
    } else {
        "SELECT id, task_id, coalesce(octet_length(metadata), 0), typeof(metadata) = 'blob'
         FROM events WHERE ?1 IS NULL ORDER BY id LIMIT ?2"
    };
    let mut statement = conn.prepare(sql)?;
    let candidates = statement
        .query_map(params![cursor, limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, usize>(2)?,
                row.get::<_, bool>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut done = candidates.len() < limit as usize;
    let mut rows = Vec::new();
    let mut bytes = 0usize;
    for row in candidates {
        if !rows.is_empty() && bytes.saturating_add(row.2) > BYTE_LIMIT {
            done = false;
            break;
        }
        bytes = bytes.saturating_add(row.2);
        rows.push(row);
    }
    Ok(Range { rows, done })
}

pub(super) fn chunk(conn: &Connection, id: i64, offset: usize) -> Result<Vec<u8>> {
    let blob = conn.blob_open(rusqlite::DatabaseName::Main, "events", "metadata", id, true)?;
    let mut bytes = vec![0; BYTE_LIMIT.min(blob.len().saturating_sub(offset))];
    blob.read_at_exact(&mut bytes, offset)?;
    Ok(bytes)
}

fn metadata(store: &Store, id: i64, bytes: usize, expected: Stamp) -> Result<Option<Vec<u8>>> {
    let mut result = Vec::new();
    while result.len() < bytes {
        let part = {
            let conn = store.db();
            if stamp(&conn)? != expected {
                return Ok(None);
            }
            chunk(&conn, id, result.len())?
        };
        anyhow::ensure!(
            !part.is_empty(),
            "Event metadata ended before its recorded byte length"
        );
        result.extend(part);
    }
    Ok(Some(result))
}

pub(super) fn scan(
    store: &Store,
    cursor: Option<i64>,
    limit: i64,
    expected: Stamp,
) -> Result<Option<Batch>> {
    let selected = {
        let conn = store.db();
        if stamp(&conn)? != expected {
            return Ok(None);
        }
        range(&conn, cursor, limit)?
    };
    let decoder = Connection::open_in_memory()?;
    let mut extract = decoder.prepare(
        "SELECT CAST(json_extract(?1, '$.api_route_launch') AS BLOB)
         WHERE ?1 LIKE '%api_route_launch%'
           AND typeof(json_extract(CASE WHEN json_valid(?1) THEN ?1 END,
                                   '$.api_route_launch')) = 'text'",
    )?;
    let mut batch = Batch {
        cursor,
        done: selected.done,
        launches: Vec::new(),
    };
    for (id, task, bytes, is_blob) in selected.rows {
        let Some(payload) = metadata(store, id, bytes, expected)? else {
            return Ok(None);
        };
        let value = if is_blob {
            rusqlite::types::ValueRef::Blob(&payload)
        } else {
            rusqlite::types::ValueRef::Text(&payload)
        };
        let route: Option<Vec<u8>> = extract.query_row([value], |row| row.get(0)).optional()?;
        if let Some(route) = route.and_then(|bytes| String::from_utf8(bytes).ok()) {
            batch.launches.push((id, task, route));
        }
        batch.cursor = Some(id);
    }
    Ok(Some(batch))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{assert_fallback, fixture};
    use super::*;
    #[test]
    fn oversized_metadata_uses_single_row_ranges_and_bounded_blob_reads() {
        let store = fixture();
        crate::store::schema::ensure_current(&store.db()).expect("migrate");
        let payload = format!(
            r#"{{"padding":"{}","api_route_launch":"large"}}"#,
            "x".repeat(BYTE_LIMIT * 12)
        );
        let conn = store.db();
        conn.execute(
            "UPDATE events SET metadata = ?1 WHERE id IN (1, 2)",
            [&payload],
        )
        .expect("large metadata");
        let selected = range(&conn, None, BATCH_SIZE).expect("range");
        assert_eq!(selected.rows.len(), 1);
        assert_eq!(selected.rows[0].0, 1);
        assert!(!selected.done);
        let mut bytes = 0;
        while bytes < payload.len() {
            let part = chunk(&conn, 1, bytes).expect("chunk");
            assert!(part.len() <= BYTE_LIMIT);
            bytes += part.len();
        }
        assert_eq!(bytes, payload.len());
        drop(conn);
        assert!(!backfill_batch(&store, BATCH_SIZE).expect("large batch"));
        assert_eq!(cursor(&store.db()).expect("bounded cursor"), Some(1));
        assert_fallback(&store);
    }

    #[test]
    fn ordinary_batches_fit_both_row_and_byte_limits() {
        let store = fixture();
        crate::store::schema::ensure_current(&store.db()).expect("migrate");
        let payload = "x".repeat(BYTE_LIMIT / 3);
        let conn = store.db();
        conn.execute("UPDATE events SET metadata = ?1", [&payload])
            .expect("payloads");
        let selected = range(&conn, None, BATCH_SIZE).expect("range");
        assert_eq!(selected.rows.len(), 3);
        assert!(selected.rows.len() <= BATCH_SIZE as usize);
        assert!(selected.rows.iter().map(|row| row.2).sum::<usize>() <= BYTE_LIMIT);
        assert!(!selected.done);
    }

    #[test]
    fn pending_tail_plan_seeks_past_the_persisted_cursor() {
        let store = fixture();
        let conn = store.db();
        let plan: String = conn
            .query_row(
                "EXPLAIN QUERY PLAN SELECT id, task_id, coalesce(octet_length(metadata), 0)
         FROM events WHERE id > ?1 ORDER BY id LIMIT ?2",
                params![3, BATCH_SIZE],
                |row| row.get(3),
            )
            .expect("plan");
        assert!(
            plan.contains("SEARCH events USING INTEGER PRIMARY KEY (rowid>?)"),
            "{plan}"
        );
    }

    #[test]
    fn maintenance_does_not_wait_for_another_writer_and_restores_busy_timeout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("launch.db");
        let fixture = fixture();
        fixture
            .db()
            .execute("VACUUM INTO ?1", [path.to_str().expect("path")])
            .expect("copy");
        let store = Store::open(&path).expect("store");
        let writer = Connection::open(&path).expect("writer");
        writer
            .execute_batch("BEGIN IMMEDIATE")
            .expect("writer lock");
        let start = Instant::now();
        assert!(!backfill_batch(&store, BATCH_SIZE).expect("skip busy maintenance"));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(cursor(&store.db()).expect("cursor"), None);
        let timeout: u64 = store
            .db()
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .expect("restored timeout");
        assert_eq!(timeout, 10_000);
        writer.execute_batch("ROLLBACK").expect("unlock");
        while !backfill_batch(&store, BATCH_SIZE).expect("resume") {}
        assert_fallback(&store);
        assert!(complete(&store.db()).expect("complete"));
    }
}
