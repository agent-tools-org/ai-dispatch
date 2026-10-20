// Memory query escaping regression coverage.
// Deps: parent query test fixtures.

use super::*;

#[test]
fn search_memories_escapes_wildcards() {
    let store = Store::open_memory().unwrap();
    let conn = store.db();
    conn.execute(
        "INSERT INTO memories (id, memory_type, content, source_task_id, agent, project_path,
         content_hash, created_at, expires_at, supersedes, version, inject_count, last_injected_at,
         success_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            "m-100-percent",
            "task_output",
            "100% success rate",
            Option::<String>::None,
            Option::<String>::None,
            Option::<String>::None,
            "hash-100-percent",
            "2026-03-15T00:00:00Z",
            Option::<String>::None,
            Option::<String>::None,
            1,
            0,
            Option::<String>::None,
            0,
        ],
    )
    .unwrap();
    drop(conn);

    let percent_results = store.search_memories("100%", None, 10, None).unwrap();
    assert_eq!(percent_results.len(), 1);
    assert_eq!(percent_results[0].content, "100% success rate");

    let plain_results = store.search_memories("100", None, 10, None).unwrap();
    assert_eq!(plain_results.len(), 1);
    assert_eq!(plain_results[0].content, "100% success rate");
}
