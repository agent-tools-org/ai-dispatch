use chrono::Local;

use super::*;

fn make_memory(id: &str, content: &str) -> Memory {
    Memory {
        id: MemoryId(id.to_string()),
        memory_type: MemoryType::Fact,
        tier: MemoryTier::OnDemand,
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
fn memory_history_returns_full_chain_from_mid_version() {
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

    let history = store.memory_history(second_id.as_str()).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(
        history.iter().map(|mem| mem.version).collect::<Vec<_>>(),
        vec![3, 2, 1]
    );
    assert_eq!(history[0].content, "third");
    assert_eq!(history[2].content, "origin");
}

#[test]
fn list_memories_by_tier_returns_identity_and_critical_memories() {
    let store = Store::open_memory().unwrap();
    let mut identity = make_memory("m-5000", "identity");
    identity.tier = MemoryTier::Identity;
    store.insert_memory(&identity).unwrap();

    let mut critical = make_memory("m-5001", "critical");
    critical.tier = MemoryTier::Critical;
    store.insert_memory(&critical).unwrap();

    let tiers = store
        .list_memories_by_tier(None, &[MemoryTier::Identity, MemoryTier::Critical])
        .unwrap();
    assert_eq!(tiers.len(), 2);
    assert!(tiers.iter().any(|memory| memory.tier == MemoryTier::Identity));
    assert!(tiers.iter().any(|memory| memory.tier == MemoryTier::Critical));
}

#[test]
fn list_memories_by_tier_excludes_on_demand_when_querying_identity() {
    let store = Store::open_memory().unwrap();
    let mut identity = make_memory("m-5100", "identity");
    identity.tier = MemoryTier::Identity;
    store.insert_memory(&identity).unwrap();

    let on_demand = make_memory("m-5101", "on-demand");
    store.insert_memory(&on_demand).unwrap();

    let tiers = store
        .list_memories_by_tier(None, &[MemoryTier::Identity])
        .unwrap();
    assert_eq!(tiers.len(), 1);
    assert_eq!(tiers[0].id.as_str(), "m-5100");
    assert_eq!(tiers[0].tier, MemoryTier::Identity);
}
