// Memory, knowledge, and context injection for agent prompts.
// Exports: inject_memories, resolve_context_from, collect_sibling_summaries, etc.
// Deps: store, types, templates, team.
use anyhow::Result;
use chrono::Local;
use std::collections::{HashMap, HashSet};

use crate::store::Store;
use crate::team::KnowledgeEntry;
use crate::templates;
use crate::types::*;

#[path = "prompt_prior.rs"]
mod prior;
pub(super) use prior::{collect_sibling_summaries, resolve_context_from};
#[cfg(test)]
use prior::{sanitize_injected_content, truncate_context_content};

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "can", "shall", "to",
    "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "about", "between",
    "through", "after", "before", "and", "but", "or", "not", "no", "if", "then", "than", "so",
    "it", "its", "this", "that", "these", "those", "all", "each", "every", "both", "few", "more",
    "most", "other", "some", "such", "only", "same", "new", "use", "used", "using", "add", "run",
    "set", "get", "code", "file", "fix", "check", "change", "make", "src", "test", "when", "how",
    "what", "which", "who", "where",
];

pub(super) fn inject_memories(store: &Store, prompt: &str, max_memories: usize, project_path: Option<&str>) -> Result<Option<(String, Vec<String>)>> {
    let Some(project_path) = project_path else { return Ok(None) };
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
    if memories.is_empty() {
        return Ok(None);
    }
    for memory in &memories {
        store.increment_memory_inject(memory.id.as_str())?;
    }

    let mut lines = vec!["[Memory]".to_string()];
    let now = Local::now();
    for mem in &memories {
        let age = format_memory_age(now.signed_duration_since(mem.created_at));
        let tier_prefix = match mem.tier {
            MemoryTier::Identity => "L0 ",
            MemoryTier::Critical => "L1 ",
            _ => "",
        };
        lines.push(format!("[{}{} {}] {}", tier_prefix, compact_type_label(&mem.memory_type), age, mem.content));
    }
    let memory_ids = memories.iter().map(|mem| mem.id.as_str().to_string()).collect();
    let token_count = templates::estimate_tokens(&lines.join("\n"));
    aid_info!("[aid] Injected {} memories (~{} tokens)", memories.len(), token_count);
    Ok(Some((lines.join("\n"), memory_ids)))
}

fn memory_tier_rank(tier: MemoryTier) -> u8 {
    match tier {
        MemoryTier::Identity => 0,
        MemoryTier::Critical => 1,
        MemoryTier::OnDemand => 2,
        MemoryTier::Deep => 3,
    }
}

pub(super) fn inject_project_state(project: &crate::project::ProjectConfig, root: &std::path::Path) -> Option<String> {
    let contents = std::fs::read_to_string(root.join(".aid/state.toml")).ok()?;
    let state: crate::state::ProjectState = toml::from_str(&contents).ok()?;
    let updated = chrono::DateTime::parse_from_rfc3339(&state.last_updated).ok()?;
    let age_days = (chrono::Utc::now() - updated.with_timezone(&chrono::Utc)).num_days();
    if age_days > 7 {
        return None;
    }
    Some(crate::state::format_state_summary_for_project(&state, &project.id))
}

pub(super) fn build_memory_queries(prompt: &str, keywords: &HashSet<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut queries = Vec::new();

    let trimmed = prompt.trim();
    if !trimmed.is_empty() {
        let truncated = trimmed.chars().take(200).collect::<String>();
        if seen.insert(truncated.clone()) {
            queries.push(truncated);
        }
    }

    let top_words = top_significant_words(prompt, keywords, 5);
    if !top_words.is_empty() {
        let joined = top_words.join(" ");
        if seen.insert(joined.clone()) {
            queries.push(joined);
        }
    }

    let paths = extract_path_tokens(prompt);
    if !paths.is_empty() {
        let joined = paths.join(" ");
        if seen.insert(joined.clone()) {
            queries.push(joined);
        }
    }

    let idents = extract_type_or_function_names(prompt);
    if !idents.is_empty() {
        let joined = idents.join(" ");
        if seen.insert(joined.clone()) {
            queries.push(joined);
        }
    }

    queries
}

pub(super) fn top_significant_words(text: &str, keywords: &HashSet<String>, limit: usize) -> Vec<String> {
    let mut counts = HashMap::new();
    for token in text.split(|c: char| !c.is_alphanumeric()) {
        let normalized = token.to_lowercase();
        if normalized.is_empty() || !keywords.contains(&normalized) {
            continue;
        }
        *counts.entry(normalized).or_insert(0) += 1;
    }
    let mut entries: Vec<_> = counts.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries.into_iter().take(limit).map(|(word, _)| word).collect()
}

pub(super) fn extract_path_tokens(prompt: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    prompt
        .split_whitespace()
        .filter_map(|token| {
            let trimmed = token.trim_matches(|c: char| matches!(c, ',' | ';' | '.' | '?' | '!' | '"' | '\'' | '[' | ']' | '{' | '}' | '(' | ')'));
            if trimmed.is_empty() || (!trimmed.contains('/') && !trimmed.contains('\\')) {
                return None;
            }
            let candidate = trimmed.to_string();
            if seen.insert(candidate.clone()) { Some(candidate) } else { None }
        })
        .collect()
}

pub(super) fn extract_type_or_function_names(prompt: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for token in prompt.split_whitespace() {
        let trimmed = token.trim_matches(|c: char| matches!(c, ',' | ';' | '.' | '?' | '!' | '"' | '\'' | '[' | ']' | '{' | '}'));
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.strip_suffix("()").unwrap_or(trimmed).to_string();
        let qualifies = trimmed.contains("::") || trimmed.ends_with("()") || key.chars().any(|c| c.is_ascii_uppercase());
        if qualifies && seen.insert(key.clone()) {
            names.push(key);
        }
    }
    names
}

pub(super) fn format_memory_age(duration: chrono::Duration) -> String {
    let days = duration.num_days();
    if days >= 30 { format!("{}mo", days / 30) }
    else if days >= 1 { format!("{}d", days) }
    else {
        let hours = duration.num_hours();
        if hours >= 1 { format!("{}h", hours) }
        else { format!("{}m", duration.num_minutes().max(1)) }
    }
}

fn compact_type_label(memory_type: &MemoryType) -> &'static str {
    match memory_type {
        MemoryType::Discovery => "D",
        MemoryType::Convention => "C",
        MemoryType::Lesson => "L",
        MemoryType::Fact => "F",
    }
}

pub(super) fn format_knowledge_block(team_id: &str, entries: &[&KnowledgeEntry]) -> String {
    let blocks: Vec<String> = entries.iter().map(|entry| format_entry_block(entry)).collect();
    format!("[Team Knowledge — {team_id}]\n{}", blocks.join("\n\n"))
}

pub(super) fn format_entry_block(entry: &KnowledgeEntry) -> String {
    let mut line = String::new();
    line.push_str("- [");
    line.push_str(&entry.topic);
    line.push(']');
    if let Some(path) = &entry.path {
        line.push('(');
        line.push_str(path);
        line.push(')');
    }
    line.push_str(" — ");
    line.push_str(&entry.description);
    if let Some(content) = &entry.content {
        line.push('\n');
        if content.len() > 500 {
            let truncated = &content[..content.floor_char_boundary(500)];
            line.push_str(truncated);
            line.push_str("...");
        } else {
            line.push_str(content);
        }
    }
    line
}

pub(super) fn select_relevant_entries<'a>(entries: &'a [KnowledgeEntry], prompt: &str) -> Vec<&'a KnowledgeEntry> {
    let prompt_words = extract_words(prompt);
    let mut scored: Vec<(usize, &KnowledgeEntry)> = entries
        .iter()
        .map(|entry| {
            let entry_words = extract_words(&format!("{} {}", entry.topic, entry.description));
            let score = entry_words.iter().filter(|word| prompt_words.contains(*word)).count();
            (score, entry)
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored
        .into_iter()
        .filter(|(score, _)| *score >= 2)
        .take(5)
        .map(|(_, entry)| entry)
        .collect()
}

pub fn extract_words(value: &str) -> HashSet<String> {
    value
        .split(|c: char| !c.is_alphanumeric())
        .filter_map(|token| {
            let normalized = token.to_lowercase();
            if normalized.is_empty() || STOP_WORDS.contains(&normalized.as_str()) {
                None
            } else {
                Some(normalized)
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "prompt_context_tests.rs"]
mod tests;
