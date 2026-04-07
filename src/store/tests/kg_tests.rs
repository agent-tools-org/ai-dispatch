// Store tests for knowledge graph mutations and queries.
// Exports: test coverage for temporal validity, search, and stats.
// Deps: Store.

use super::*;

#[test]
fn add_and_query_entity() {
    let store = Store::open_memory().unwrap();
    let triple_id = store
        .add_kg_triple("pool.rs", "has_bug", "race_condition", None, Some("t-abc1"))
        .unwrap();

    let triples = store.query_kg_entity("pool.rs", None).unwrap();
    assert_eq!(triples.len(), 1);
    assert_eq!(triples[0].id, triple_id);
    assert_eq!(triples[0].predicate, "has_bug");
    assert_eq!(triples[0].object, "race_condition");
    assert_eq!(triples[0].source.as_deref(), Some("t-abc1"));
}

#[test]
fn invalidate_triple_sets_valid_to() {
    let store = Store::open_memory().unwrap();
    let triple_id = store
        .add_kg_triple("auth_module", "uses", "bcrypt", None, None)
        .unwrap();

    assert!(store.invalidate_kg_triple(triple_id).unwrap());

    let triples = store.query_kg_entity("auth_module", None).unwrap();
    assert_eq!(triples.len(), 1);
    assert!(triples[0].valid_to.is_some());
}

#[test]
fn query_respects_temporal_validity() {
    let store = Store::open_memory().unwrap();
    store
        .add_kg_triple(
            "scheduler",
            "uses",
            "backoff_v2",
            Some("2026-04-10"),
            None,
        )
        .unwrap();

    let before = store
        .query_kg_entity("scheduler", Some("2026-04-09"))
        .unwrap();
    let after = store
        .query_kg_entity("scheduler", Some("2026-04-10"))
        .unwrap();
    assert!(before.is_empty());
    assert_eq!(after.len(), 1);
}

#[test]
fn timeline_returns_chronological() {
    let store = Store::open_memory().unwrap();
    store
        .add_kg_triple("api", "depends_on", "db", Some("2026-04-01"), None)
        .unwrap();
    store
        .add_kg_triple("api", "depends_on", "cache", Some("2026-04-02"), None)
        .unwrap();
    store
        .add_kg_triple("api", "depends_on", "queue", Some("2026-04-03"), None)
        .unwrap();

    let timeline = store.kg_timeline("api").unwrap();
    let objects = timeline
        .iter()
        .map(|triple| triple.object.as_str())
        .collect::<Vec<_>>();
    assert_eq!(objects, vec!["db", "cache", "queue"]);
}

#[test]
fn search_matches_across_fields() {
    let store = Store::open_memory().unwrap();
    store
        .add_kg_triple("worker_pool", "uses", "redis", None, Some("task-a"))
        .unwrap();
    store
        .add_kg_triple("scheduler", "depends_on", "queue", None, Some("task-b"))
        .unwrap();

    assert_eq!(store.search_kg("worker_pool").unwrap().len(), 1);
    assert_eq!(store.search_kg("depends_on").unwrap().len(), 1);
    assert_eq!(store.search_kg("queue").unwrap().len(), 1);
    assert_eq!(store.search_kg("task-a").unwrap().len(), 1);
}

#[test]
fn stats_counts_entities_and_triples() {
    let store = Store::open_memory().unwrap();
    store
        .add_kg_triple("api", "depends_on", "db", None, None)
        .unwrap();
    let invalidated = store
        .add_kg_triple("api", "depends_on", "cache", None, None)
        .unwrap();
    assert!(store.invalidate_kg_triple(invalidated).unwrap());

    let stats = store.kg_stats().unwrap();
    assert_eq!(stats.triple_count, 2);
    assert_eq!(stats.active_triple_count, 1);
    assert_eq!(stats.entity_count, 3);
    assert_eq!(stats.predicate_count, 1);
}
