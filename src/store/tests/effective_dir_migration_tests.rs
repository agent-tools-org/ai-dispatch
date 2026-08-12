// Migration: backfill effective_dir from pre-column dispatch_args.dir.
// Starts from a schema that lacks the column; proves a legacy --dir task
// can resolve its report after Store::open().
// Deps: Store, RunArgs, read_task_output, AidHomeGuard.

use crate::cmd::run::RunArgs;
use crate::cmd::show::{missing_owned_output_absence, owned_output_path, read_task_output};
use crate::paths::AidHomeGuard;
use crate::store::Store;
use rusqlite::Connection;
use std::path::Path;

fn create_pre_effective_dir_database(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            agent TEXT NOT NULL,
            prompt TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            worktree_path TEXT,
            worktree_branch TEXT,
            log_path TEXT,
            output_path TEXT,
            tokens INTEGER,
            duration_ms INTEGER,
            created_at TEXT NOT NULL,
            completed_at TEXT,
            dispatch_args TEXT
        );
        CREATE TABLE events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            event_type TEXT NOT NULL,
            detail TEXT NOT NULL
        );",
    )
    .unwrap();
}

fn insert_legacy_task(conn: &Connection, id: &str, output: &str, dispatch_args: Option<&str>) {
    conn
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at, output_path, dispatch_args)
             VALUES (?1, 'codex', 'audit', 'done', '2026-08-01T00:00:00+00:00', ?2, ?3)",
            rusqlite::params![id, output, dispatch_args],
        )
        .unwrap();
}

fn has_column(conn: &Connection, name: &str) -> bool {
    let mut stmt = conn.prepare("PRAGMA table_info(tasks)").unwrap();
    stmt.query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .any(|col| col.ok().as_deref() == Some(name))
}

fn insert_legacy_rows(conn: &Connection, dir: &Path) {
    let args = RunArgs {
        dir: Some(dir.display().to_string()),
        output: Some("report.md".to_string()),
        ..Default::default()
    };
    insert_legacy_task(
        conn,
        "t-legacy-dir",
        "report.md",
        Some(&args.dispatch_args_json().unwrap()),
    );
    insert_legacy_task(conn, "t-legacy-nodir", "report.md", Some(r#"{"prompt":"research"}"#));
    insert_legacy_task(conn, "t-legacy-rel", "report.md", Some(r#"{"dir":"."}"#));
    insert_legacy_task(
        conn,
        "t-legacy-missing-dir",
        "report.md",
        Some(r#"{"dir":"/tmp/aid-dir-that-no-longer-exists"}"#),
    );
}

fn assert_legacy_rows_resolve(store: &Store, dir: &Path) {
    let owner = store.get_task("t-legacy-dir").unwrap().unwrap();
    assert_eq!(owner.effective_dir.as_deref(), Some(dir.to_str().unwrap()));
    assert_eq!(read_task_output(&owner).unwrap(), "LEGACY_DIR_OWNED_REPORT\n");

    let no_dir = store.get_task("t-legacy-nodir").unwrap().unwrap();
    assert!(no_dir.effective_dir.is_none());
    assert_eq!(owned_output_path(&no_dir), None);
    assert!(
        missing_owned_output_absence(&no_dir)
            .unwrap()
            .contains("No task-owned output file"),
        "absence must stay honest when no directory was recorded"
    );

    let relative = store.get_task("t-legacy-rel").unwrap().unwrap();
    assert!(relative.effective_dir.is_none(), "relative --dir is not usable without CWD");

    let missing_dir = store.get_task("t-legacy-missing-dir").unwrap().unwrap();
    assert_eq!(
        missing_dir.effective_dir.as_deref(),
        Some("/tmp/aid-dir-that-no-longer-exists")
    );
    assert!(read_task_output(&missing_dir).is_err());
}

/// The defect: a bare ADD COLUMN left every pre-migration `--dir` row NULL, so
/// a relative `-o report.md` could never be resolved after CWD was removed.
#[test]
fn migrate_backfills_effective_dir_so_legacy_dir_task_resolves_report() {
    let root = tempfile::tempdir().unwrap();
    let aid_home = root.path().join("aid-home");
    let _aid_home = AidHomeGuard::set(&aid_home);
    let dir = root.path().join("audit-dir");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("report.md"), "LEGACY_DIR_OWNED_REPORT\n").unwrap();

    let db_path = root.path().join("aid.db");
    create_pre_effective_dir_database(&db_path);
    let conn = Connection::open(&db_path).unwrap();
    assert!(!has_column(&conn, "effective_dir"), "fixture must start without the column");
    insert_legacy_rows(&conn, &dir);
    drop(conn);

    let store = Store::open(&db_path).unwrap();
    assert_legacy_rows_resolve(&store, &dir);

    store
        .db()
        .execute(
            "UPDATE tasks SET effective_dir = ?1 WHERE id = 't-legacy-dir'",
            rusqlite::params!["/tmp/already-populated-dir"],
        )
        .unwrap();
    drop(store);
    let reopened = Store::open(&db_path).unwrap();
    let owner = reopened.get_task("t-legacy-dir").unwrap().unwrap();
    assert_eq!(owner.effective_dir.as_deref(), Some("/tmp/already-populated-dir"));
}

#[test]
fn malformed_dispatch_args_do_not_abort_open_or_skip_valid_sibling() {
    let root = tempfile::tempdir().unwrap();
    let db_path = root.path().join("aid.db");
    create_pre_effective_dir_database(&db_path);
    let conn = Connection::open(&db_path).unwrap();
    insert_legacy_task(
        &conn,
        "t-valid-dir",
        "report.md",
        Some(r#"{"dir":"/tmp/valid-historical-dir"}"#),
    );
    insert_legacy_task(
        &conn,
        "t-whitespace-dir",
        "report.md",
        Some(r#"{"dir":"/tmp/recorded-dir-with-space "}"#),
    );
    insert_legacy_task(&conn, "t-malformed", "report.md", Some("{not json"));
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at, output_path, dispatch_args)
         VALUES (X'00FF', 'codex', 'audit', 'done', '2026-08-01T00:00:00+00:00', 'report.md', ?1)",
        rusqlite::params![r#"{"dir":"/tmp/non-text-id-dir"}"#],
    )
    .unwrap();
    drop(conn);

    let store = Store::open(&db_path).unwrap();
    let valid = store.get_task("t-valid-dir").unwrap().unwrap();
    assert_eq!(valid.effective_dir.as_deref(), Some("/tmp/valid-historical-dir"));
    let whitespace = store.get_task("t-whitespace-dir").unwrap().unwrap();
    assert_eq!(
        whitespace.effective_dir.as_deref(),
        Some("/tmp/recorded-dir-with-space")
    );
    let malformed = store.get_task("t-malformed").unwrap().unwrap();
    assert!(malformed.effective_dir.is_none());
    let non_text_id_dir: Option<String> = store
        .db()
        .query_row(
            "SELECT effective_dir FROM tasks WHERE typeof(id) = 'blob'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(non_text_id_dir.is_none());
}
