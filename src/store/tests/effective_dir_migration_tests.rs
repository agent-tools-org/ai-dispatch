// Migration: backfill effective_dir from pre-column dispatch_args.dir.
// Starts from a schema that lacks the column; proves a legacy --dir task
// can resolve its report after migrate().
// Deps: Store, RunArgs, read_task_output, AidHomeGuard.

use crate::cmd::run::RunArgs;
use crate::cmd::show::{missing_owned_output_absence, owned_output_path, read_task_output};
use crate::paths::AidHomeGuard;
use crate::store::Store;

fn pre_effective_dir_store() -> Store {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
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
    Store {
        conn: std::sync::Mutex::new(conn),
    }
}

fn insert_legacy_task(store: &Store, id: &str, output: &str, dispatch_args: Option<&str>) {
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at, output_path, dispatch_args)
             VALUES (?1, 'codex', 'audit', 'done', '2026-08-01T00:00:00+00:00', ?2, ?3)",
            rusqlite::params![id, output, dispatch_args],
        )
        .unwrap();
}

fn has_column(store: &Store, name: &str) -> bool {
    let conn = store.db();
    let mut stmt = conn.prepare("PRAGMA table_info(tasks)").unwrap();
    stmt.query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .any(|col| col.ok().as_deref() == Some(name))
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

    let store = pre_effective_dir_store();
    assert!(
        !has_column(&store, "effective_dir"),
        "fixture must start without the column"
    );

    let args = RunArgs {
        dir: Some(dir.display().to_string()),
        output: Some("report.md".to_string()),
        ..Default::default()
    };
    insert_legacy_task(
        &store,
        "t-legacy-dir",
        "report.md",
        Some(&args.dispatch_args_json().unwrap()),
    );
    insert_legacy_task(
        &store,
        "t-legacy-nodir",
        "report.md",
        Some(r#"{"prompt":"research"}"#),
    );
    insert_legacy_task(
        &store,
        "t-legacy-rel",
        "report.md",
        Some(r#"{"dir":"."}"#),
    );

    store.migrate().unwrap();

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
    assert!(
        relative.effective_dir.is_none(),
        "relative --dir is not usable without CWD"
    );
}
