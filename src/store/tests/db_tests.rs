// Store database configuration tests.
// Verifies concurrency pragmas against an in-memory connection.
// Deps: crate::store, tempfile.

use crate::store::optimize_for_concurrency;

#[test]
fn optimize_for_concurrency_supports_in_memory_db() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    optimize_for_concurrency(&conn).unwrap();
}

