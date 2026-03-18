// Store database configuration and stats tests.
// Exports: concurrency pragma and db stats tests.
// Deps: crate::store, tempfile.

use crate::store::{Store, optimize_for_concurrency};

#[test]
fn optimize_for_concurrency_supports_in_memory_db() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    optimize_for_concurrency(&conn).unwrap();
}

#[test]
fn db_stats_returns_valid_values() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("aid.db")).unwrap();

    let stats = store.db_stats().unwrap();
    assert!(stats.free_pages <= stats.page_count);
    assert!(stats.page_count > 0);
    assert!(stats.size_bytes > 0);
}
