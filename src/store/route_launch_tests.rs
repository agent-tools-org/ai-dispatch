// Regression coverage for launch equivalence, migration DDL, cursor bounds and atomic writes.
// Exercises real SQLite databases through Store and the historical fallback query.
use super::*;
use crate::store::Store;

fn fixture() -> Store {
    let store = Store::open_memory().expect("store");
    let conn = store.db();
    for id in ["a", "b", "c"] {
        conn.execute(
            "INSERT INTO tasks(id, agent, prompt, created_at) VALUES (?1, 'codex', '', 'now')",
            [id],
        )
        .expect("task");
    }
    store_v2(&conn);
    for (task, metadata) in [
        ("a", r#"{"api_route_launch":"one"}"#),
        ("a", r#"{"api_route_launch":"one"}"#),
        ("a", r#"{"api_route_launch":"two"}"#),
        ("b", r#"{"api_route_launch":"one"}"#),
        ("b", "malformed api_route_launch"),
        ("b", r#"{"other":"api_route_launch"}"#),
        ("c", r#"{"api_route_launch":12}"#),
        ("c", r#"{"api_route_launch":null}"#),
        ("c", r#"{"api_route_launch":{"x":1}}"#),
        ("c", r#"{"api\u005froute_launch":"escaped"}"#),
    ] {
        conn.execute(
            "INSERT INTO events(task_id,timestamp,event_type,detail,metadata)
             VALUES (?1,'now','setup','',?2)",
            params![task, metadata],
        )
        .expect("event");
    }
    drop(conn);
    store
}

fn store_v2(conn: &Connection) {
    conn.execute_batch(
        "DROP TRIGGER record_api_route_launch;
         DROP TABLE api_route_launches;
         DROP TABLE api_route_launch_backfill;
         CREATE INDEX IF NOT EXISTS idx_events_task_id_id ON events(task_id,id);
         PRAGMA user_version = 2;",
    )
    .expect("v2");
}

fn ids(conn: &Connection, sql: &str, route: &str) -> Vec<String> {
    let mut result = conn
        .prepare(sql)
        .expect("prepare")
        .query_map([route], |r| r.get(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<String>>>()
        .expect("rows");
    result.sort();
    result
}

fn equivalent(conn: &Connection, sql: &str) {
    for route in ["one", "two", "missing", "12", "escaped", r#"{"x":1}"#] {
        assert_eq!(ids(conn, sql, route), ids(conn, LEGACY, route), "{route}");
    }
}

#[test]
fn v2_migration_is_ddl_only_and_drops_duplicate_index() {
    let store = fixture();
    let conn = store.db();
    let before = conn.total_changes();
    crate::store::schema::ensure_current(&conn).expect("migrate");
    // Only the singleton backfill state is written; no historical evidence is read or written.
    assert_eq!(conn.total_changes() - before, 1);
    assert_eq!(ids(&conn, LOOKUP, "one"), Vec::<String>::new());
    assert_eq!(ids(&conn, LEGACY, "one"), ["a", "b"]);
    let duplicate: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name = 'idx_events_task_id_id'",
            [],
            |r| r.get(0),
        )
        .expect("index");
    assert_eq!(duplicate, 0);
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(version, 3);
}

#[test]
fn bounded_backfill_resumes_and_preserves_lookup_equivalence() {
    let store = fixture();
    let mut conn = store.db();
    crate::store::schema::ensure_current(&conn).expect("migrate");
    assert_fallback(&mut conn);
    assert!(!backfill_batch(&mut conn, 3).expect("first batch"));
    let cursor: i64 = conn
        .query_row("SELECT cursor FROM api_route_launch_backfill", [], |r| {
            r.get(0)
        })
        .expect("cursor");
    assert_eq!(cursor, 3);
    assert_fallback(&mut conn);
    drop(conn);
    let mut conn = store.db();
    assert!(!backfill_batch(&mut conn, 3).expect("resume"));
    let resumed: i64 = conn
        .query_row("SELECT cursor FROM api_route_launch_backfill", [], |r| {
            r.get(0)
        })
        .expect("resumed cursor");
    assert_eq!(resumed, 6);
    while !backfill_batch(&mut conn, 3).expect("finish") {}
    assert!(complete(&conn).expect("complete"));
    equivalent(&conn, LOOKUP);
    drop(conn);
    assert_eq!(
        store.route_launch_task_ids("one").expect("public lookup"),
        ["a", "b"]
    );
}

#[test]
fn completed_lookup_uses_route_index_without_events_scan() {
    let store = fixture();
    let mut conn = store.db();
    crate::store::schema::ensure_current(&conn).expect("migrate");
    while !backfill_batch(&mut conn, 3).expect("batch") {}
    let plans = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {LOOKUP}"))
        .expect("plan")
        .query_map(["one"], |r| r.get::<_, String>(3))
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("plans")
        .join("\n");
    assert!(plans.contains("idx_api_route_launches_route"), "{plans}");
    assert!(!plans.contains("events"), "{plans}");
}

#[test]
fn new_evidence_is_atomic_with_event_insertion() {
    let store = fixture();
    let conn = store.db();
    crate::store::schema::ensure_current(&conn).expect("migrate");
    conn.execute_batch("BEGIN IMMEDIATE").expect("begin");
    conn.execute(
        "INSERT INTO events(task_id,timestamp,event_type,detail,metadata)
         VALUES ('b','now','setup','', '{\"api_route_launch\":\"new\"}')",
        [],
    )
    .expect("insert");
    assert_eq!(ids(&conn, LOOKUP, "new"), ["b"]);
    conn.execute_batch("ROLLBACK").expect("rollback");
    assert!(ids(&conn, LOOKUP, "new").is_empty());
    assert!(ids(&conn, LEGACY, "new").is_empty());
}

fn assert_fallback(conn: &mut Connection) {
    for route in ["one", "two", "missing", "12", "escaped", r#"{"x":1}"#] {
        let mut actual = lookup_with_budget(conn, route, Duration::ZERO).expect("fallback");
        actual.sort();
        assert_eq!(actual, ids(conn, LEGACY, route));
    }
}

#[test]
fn backfill_cursor_survives_database_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("launch.db");
    let store = fixture();
    {
        let conn = store.db();
        conn.execute("VACUUM INTO ?1", [path.to_str().expect("path")])
            .expect("copy");
    }
    let store = Store::open(&path).expect("migrate file");
    assert!(!backfill_batch(&mut store.db(), 3).expect("batch"));
    drop(store);
    let store = Store::open(&path).expect("reopen");
    let mut conn = store.db();
    let cursor: i64 = conn
        .query_row("SELECT cursor FROM api_route_launch_backfill", [], |r| {
            r.get(0)
        })
        .expect("cursor");
    assert_eq!(cursor, 3);
    assert_fallback(&mut conn);
    while !backfill_batch(&mut conn, 3).expect("finish") {}
    equivalent(&conn, LOOKUP);
}
