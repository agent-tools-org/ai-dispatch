// Regression tests for spec-less orphan cleanup.
// Covers stale failure and recent-activity retention without a background spec.
// Deps: background orphan cleanup, shared test task/event helpers, Store, and paths.

use super::{cleanup_orphaned_idle_tasks, insert_event, make_task};
use crate::types::{EventKind, TaskStatus};
use crate::{paths, store::Store};

struct TestHome {
    _temp: tempfile::TempDir,
    _guard: paths::AidHomeGuard,
}

fn setup_home() -> TestHome {
    let temp = tempfile::tempdir().expect("tempdir");
    let guard = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    TestHome {
        _temp: temp,
        _guard: guard,
    }
}

#[test]
fn orphan_reaper_fails_stale_task_without_background_spec() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    let task = make_task("t-nospec-stale");
    store.insert_task(&task).expect("insert task");
    insert_event(&store, "t-nospec-stale", 1_000, EventKind::Milestone);

    let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|_| false).expect("cleanup");

    assert_eq!(cleaned, vec!["t-nospec-stale".to_string()]);
    assert_eq!(
        store.get_task("t-nospec-stale").expect("get").expect("task").status,
        TaskStatus::Failed
    );
    assert!(store.get_events("t-nospec-stale").expect("events").iter()
        .any(|event| event.detail.contains("orphaned supervisor")));
}

#[test]
fn orphan_reaper_keeps_recent_task_without_background_spec() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    let task = make_task("t-nospec-recent");
    store.insert_task(&task).expect("insert task");
    insert_event(&store, "t-nospec-recent", 1, EventKind::Milestone);

    let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|_| false).expect("cleanup");

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-nospec-recent").expect("get").expect("task").status,
        TaskStatus::Running
    );
}
