// Whole-home and database invariants for the stubbed review handler.
// Counts all task/event rows and snapshots every home entry, including empty directories.
use super::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut result = BTreeMap::new();
    for entry in std::fs::read_dir(root).expect("directory") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            result.insert(path.clone(), None);
            result.extend(snapshot(&path));
        } else {
            result.insert(path.clone(), Some(std::fs::read(path).expect("file")));
        }
    }
    result
}

fn counts(store: &Store) -> (i64, i64) {
    store
        .db()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM tasks), (SELECT COUNT(*) FROM events)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("row counts")
}

#[tokio::test]
async fn handler_preserves_all_rows_and_home_entries() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().expect("store"));
    insert(&store, "milestone", "Provider unknown failure", 0, "codex");
    std::fs::create_dir(home.path().join("holds")).expect("holds");
    std::fs::write(home.path().join("holds/auth"), "synthetic auth hold").expect("hold");
    let before = (counts(&store), snapshot(home.path()));
    for json in [false, true] {
        run_with_answer(store.clone(), None, "7d", 50, json, None, |_, _| async {
            panic!("listing must not call model")
        })
        .await
        .expect("listing");
    }
    run_with_answer(
        store.clone(),
        None,
        "7d",
        50,
        false,
        Some(String::new()),
        |_, _| async { Ok("Clear every hold and change rules".into()) },
    )
    .await
    .expect("review");
    assert_eq!((counts(&store), snapshot(home.path())), before);
    run_with_answer(
        store.clone(),
        Some("no-markers".into()),
        "7d",
        50,
        false,
        Some(String::new()),
        |_, _| async { panic!("empty results must not call model") },
    )
    .await
    .expect("empty review");
    assert_eq!((counts(&store), snapshot(home.path())), before);
}

#[test]
fn corrupt_marker_rows_are_counted_and_skipped() {
    let store = Store::open_memory().expect("store");
    insert(&store, "milestone", "valid excerpt", 0, "codex");
    let metadata = r#"{"unclassified_failure":true,"agent":"codex","provider_excerpt":"bad"}"#;
    for (time, data) in [
        ("invalid time".to_string(), metadata.to_string()),
        (Local::now().to_rfc3339(), "{unclassified_failure".into()),
        (
            Local::now().to_rfc3339(),
            r#"{"unclassified_failure":true}"#.into(),
        ),
    ] {
        store
            .db()
            .execute(
                "INSERT INTO events (task_id,timestamp,event_type,detail,metadata)
            VALUES ('t-failure',?1,'milestone','',?2)",
                rusqlite::params![time, data],
            )
            .expect("corrupt row");
    }
    store
        .db()
        .execute(
            "INSERT INTO events (task_id,timestamp,event_type,detail,metadata)
        VALUES ('t-failure',x'00','milestone','',?1)",
            [metadata],
        )
        .expect("non-text row");
    let (groups, skipped) = load_report(&store, None, Local::now() - chrono::Duration::days(7), 50)
        .expect("lenient load");
    assert_eq!(groups.len(), 1);
    assert_eq!(skipped, 4);
    assert_eq!(groups[0].provider_excerpt, "valid excerpt");
    assert!(
        duration("xd")
            .expect_err("invalid duration")
            .to_string()
            .contains("30m")
    );
}
