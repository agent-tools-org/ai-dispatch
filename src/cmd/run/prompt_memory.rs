// Memory selection for prompt assembly, including tiers and relevance scoring.
// Returns owned entries; rendering and bounds remain in the parent pipeline.
use super::*;

pub(super) fn select_memories(
    store: &Store,
    prompt: &str,
    max_memories: usize,
    project_path: &str,
) -> Result<Vec<Memory>> {
    let mut always_memories = store.list_memories_by_tier(
        Some(project_path),
        &[MemoryTier::Identity, MemoryTier::Critical],
    )?;
    always_memories.sort_by(|a, b| {
        memory_tier_rank(a.tier)
            .cmp(&memory_tier_rank(b.tier))
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    let always_memories: Vec<_> = always_memories.into_iter().take(max_memories).collect();
    let keywords = extract_words(prompt);
    let queries = build_memory_queries(prompt, &keywords);
    let mut scored: HashMap<String, (Memory, usize)> = HashMap::new();
    for query in queries {
        for memory in store.search_memories(
            &query,
            Some(project_path),
            max_memories,
            Some(&[MemoryTier::OnDemand]),
        )? {
            let entry = scored
                .entry(memory.id.as_str().to_string())
                .or_insert((memory.clone(), 0));
            entry.1 += 1;
        }
    }

    let mut scored: Vec<_> = scored.into_values().collect();
    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.0.created_at.cmp(&a.0.created_at))
    });
    let remaining_slots = max_memories.saturating_sub(always_memories.len());
    let mut memories = always_memories;
    memories.extend(
        scored
            .into_iter()
            .take(remaining_slots)
            .map(|(memory, _)| memory),
    );
    Ok(memories)
}
