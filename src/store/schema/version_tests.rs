// File-backed regression tests for schema admission and historical upgrades.
// Uses real SQLite locks and legacy DDL, independently of Store creation.
use super::version::CURRENT_VERSION;
use crate::store::Store;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

const OLD_TASKS: &str = "CREATE TABLE tasks (
    id TEXT PRIMARY KEY, agent TEXT NOT NULL, prompt TEXT NOT NULL,
    resolved_prompt TEXT, status TEXT NOT NULL DEFAULT 'pending', parent_task_id TEXT,
    workgroup_id TEXT, caller_kind TEXT, caller_session_id TEXT, repo_path TEXT,
    worktree_path TEXT, worktree_branch TEXT, start_sha TEXT, log_path TEXT, output_path TEXT,
    tokens INTEGER, duration_ms INTEGER, model TEXT, cost_usd REAL, created_at TEXT NOT NULL,
    started_at TEXT, completed_at TEXT, completion_summary TEXT, peer_review TEXT,
    verify_status TEXT NOT NULL DEFAULT 'skipped'
);";

fn legacy(path: &Path, version: i64) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(OLD_TASKS).unwrap();
    conn.execute_batch(
        "INSERT INTO tasks (id, agent, prompt, created_at, verify_status)
         VALUES ('old', 'codex', 'preserved', 'now', 'empty_diff'),
                ('hollow', 'codex', 'preserved', 'now', 'hollow_output');",
    )
    .unwrap();
    conn.pragma_update(None, "user_version", version).unwrap();
}

fn schema(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
        )
        .unwrap();
    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let mut result = Vec::new();
    for table in tables {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        let mut columns: Vec<String> = stmt
            .query_map([], |row| {
                Ok(format!(
                    "{table}:{}:{}:{}:{:?}:{}",
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        columns.sort();
        result.extend(columns);
    }
    let mut stmt = conn
        .prepare(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND sql IS NOT NULL ORDER BY name",
        )
        .unwrap();
    result.extend(
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap(),
    );
    result
}

#[test]
fn current_open_reads_while_writer_holds_admission() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aid.db");
    let writer = Store::open(&path).unwrap();
    writer.db().execute_batch("BEGIN IMMEDIATE").unwrap();
    let start = Instant::now();
    let reader = Store::open(&path).unwrap();
    assert!(
        reader
            .list_tasks(crate::types::TaskFilter::All)
            .unwrap()
            .is_empty()
    );
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(reader.db().total_changes(), 0);
    writer.db().execute_batch("ROLLBACK").unwrap();
}

#[test]
fn historical_versions_converge_to_fresh_schema_and_normalize_once() {
    let fresh = Store::open_memory().unwrap();
    let expected = schema(&fresh.db());
    for version in [0, 1] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("aid.db");
        legacy(&path, version);
        let store = Store::open(&path).unwrap();
        assert_eq!(schema(&store.db()), expected);
        let rows: Vec<(String, String, String)> = store
            .db()
            .prepare("SELECT prompt, verify_status, delivery_assessment FROM tasks ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("preserved".into(), "skipped".into(), "hollow_output".into()),
                ("preserved".into(), "skipped".into(), "empty_diff".into()),
            ]
        );
        assert_normalizes_once(store, &path);
    }
}

#[test]
fn concurrent_old_opens_migrate_only_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aid.db");
    legacy(&path, 1);
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "PRAGMA journal_mode=WAL;
         CREATE TABLE migration_count (n INTEGER NOT NULL);
         INSERT INTO migration_count VALUES (0);
         CREATE TRIGGER count_normalization AFTER UPDATE OF verify_status ON tasks
         BEGIN UPDATE migration_count SET n = n + 1; END;",
        )
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Store::open(&path).unwrap()
            })
        })
        .collect();
    let stores: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(
        stores[0]
            .db()
            .query_row("SELECT n FROM migration_count", [], |r| r.get::<_, i64>(0),)
            .unwrap(),
        2
    );
    assert_eq!(
        stores[1]
            .db()
            .total_changes()
            .min(stores[0].db().total_changes()),
        0
    );
}

fn assert_normalizes_once(store: Store, path: &Path) {
    assert_eq!(
        store
            .db()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        CURRENT_VERSION
    );
    store
        .db()
        .execute_batch("UPDATE tasks SET verify_status = 'empty_diff', delivery_assessment = NULL")
        .unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store
            .db()
            .query_row("SELECT verify_status FROM tasks LIMIT 1", [], |r| r
                .get::<_, String>(0),)
            .unwrap(),
        "empty_diff"
    );
}
