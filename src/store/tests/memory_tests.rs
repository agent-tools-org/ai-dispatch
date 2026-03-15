use chrono::Local;

use crate::types::*;
use super::*;

fn make_memory(id: &str, content: &str) -> Memory {
    Memory {
        id: MemoryId(id.to_string()),
        memory_type: MemoryType::Fact,
        content: content.to_string(),
        source_task_id: None,
        agent: None,
        project_path: None,
        content_hash: format!("hash-{}", id),
        created_at: Local::now(),
        expires_at: None,
        supersedes: None,
        version: 1,
        inject_count: 0,
        last_injected_at: None,
        success_count: 0,
    }
}

#[test]
fn insert_and_update_memory_creates_new_version() {
    let store = Store::open_memory().unwrap();
    let base = make_memory("m-1100", "initial");
    store.insert_memory(&base).unwrap();
    assert!(store.update_memory(base.id.as_str(), "updated").unwrap());

    let memories = store.list_memories(None, None).unwrap();
    assert_eq!(memories.len(), 1);
    let updated = &memories[0];
    assert_eq!(updated.version, 2);
    assert_eq!(updated.content, "updated");
    assert_eq!(
        updated.supersedes.as_ref().map(|id| id.as_str()),
        Some(base.id.as_str())
    );
}

#[test]
fn list_memories_returns_only_latest_versions() {
    let store = Store::open_memory().unwrap();
    let first = make_memory("m-2100", "first");
    store.insert_memory(&first).unwrap();
    assert!(store.update_memory(first.id.as_str(), "first-updated").unwrap());

    let second = make_memory("m-2200", "second");
    store.insert_memory(&second).unwrap();

    let memories = store.list_memories(None, None).unwrap();
    assert_eq!(memories.len(), 2);
    let updated_chain_count = memories
        .iter()
        .filter(|mem| mem.supersedes.is_some())
        .count();
    assert_eq!(updated_chain_count, 1);
}

#[test]
fn increment_memory_counters_updates_usage() {
    let store = Store::open_memory().unwrap();
    let memory = make_memory("m-3000", "counter");
    store.insert_memory(&memory).unwrap();

    assert!(store.increment_memory_inject(memory.id.as_str()).unwrap());
    assert!(store.increment_memory_success(memory.id.as_str()).unwrap());

    let loaded = store.list_memories(None, None).unwrap();
    let record = &loaded[0];
    assert_eq!(record.inject_count, 1);
    assert!(record.last_injected_at.is_some());
    assert_eq!(record.success_count, 1);
}

#[test]
fn list_memory_history_returns_complete_chain() {
    let store = Store::open_memory().unwrap();
    let origin = make_memory("m-4000", "origin");
    store.insert_memory(&origin).unwrap();

    assert!(store.update_memory(origin.id.as_str(), "second").unwrap());
    let second_id = store
        .list_memories(None, None)
        .unwrap()
        .first()
        .unwrap()
        .id
        .clone();

    assert!(store.update_memory(second_id.as_str(), "third").unwrap());
    let final_id = store
        .list_memories(None, None)
        .unwrap()
        .first()
        .unwrap()
        .id
        .clone();

    let history = store.list_memory_history(final_id.as_str()).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].version, 1);
    assert_eq!(history[1].version, 2);
    assert_eq!(history[2].version, 3);
    assert_eq!(history[2].content, "third");
}
