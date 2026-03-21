// Memory, knowledge, and context injection for agent prompts.
// Exports: inject_memories, resolve_context_from, collect_sibling_summaries, etc.
// Deps: store, types, templates, team.
use anyhow::Result;
use chrono::Local;
use serde_json;
use std::collections::{HashMap, HashSet};

use crate::cmd::show::extract_messages_from_log;
use crate::cmd::summary::CompletionSummary;
use crate::store::Store;
use crate::team::KnowledgeEntry;
use crate::templates;
use crate::types::*;

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

pub(super) fn inject_memories(store: &Store, prompt: &str, max_memories: usize) -> Result<Option<(String, Vec<String>)>> {
    let project_path = detect_project_path();
    let keywords = extract_words(prompt);
    let queries = build_memory_queries(prompt, &keywords);
    if queries.is_empty() {
        return Ok(None);
    }

    let mut scored: HashMap<String, (Memory, usize)> = HashMap::new();
    for query in queries {
        for memory in store.search_memories(&query, project_path.as_deref(), max_memories)? {
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
    let memories: Vec<_> = scored.into_iter().take(max_memories).map(|(mem, _)| mem).collect();
    if memories.is_empty() {
        return Ok(None);
    }
    for memory in &memories {
        store.increment_memory_inject(memory.id.as_str())?;
    }

    let mut lines = vec!["[Agent Memory — knowledge from past tasks]".to_string()];
    let now = Local::now();
    for mem in &memories {
        let age = format_memory_age(now.signed_duration_since(mem.created_at));
        lines.push(format!("- [{}] ({}) {}", mem.memory_type.label(), age, mem.content));
    }
    let memory_ids = memories.iter().map(|mem| mem.id.as_str().to_string()).collect();
    let token_count = templates::estimate_tokens(&lines.join("\n"));
    aid_info!("[aid] Injected {} memories (~{} tokens)", memories.len(), token_count);
    Ok(Some((lines.join("\n"), memory_ids)))
}

pub(super) fn inject_project_state() -> Option<String> {
    let state = crate::state::load_state().ok()??;
    let updated = chrono::DateTime::parse_from_rfc3339(&state.last_updated).ok()?;
    let age_days = (chrono::Utc::now() - updated.with_timezone(&chrono::Utc)).num_days();
    if age_days > 7 {
        return None;
    }
    Some(crate::state::format_state_summary(&state))
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
    if days >= 30 { format!("{}mo ago", days / 30) }
    else if days >= 1 { format!("{}d ago", days) }
    else {
        let hours = duration.num_hours();
        if hours >= 1 { format!("{}h ago", hours) }
        else { format!("{}m ago", duration.num_minutes().max(1)) }
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
    scored.sort_by(|a, b| b.0.cmp(&a.0));
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

pub(super) fn detect_project_path() -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| if o.status.success() {
            String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
        } else {
            None
        })
}

fn sanitize_injected_content(content: &str) -> String {
    let mut result = Vec::new();
    let mut inside = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("<aid-") && !trimmed.starts_with("</aid-") {
            inside = true;
            continue;
        }
        if trimmed.starts_with("</aid-") {
            inside = false;
            continue;
        }
        if !inside {
            result.push(line);
        }
    }
    result.join("\n")
}

fn truncate_context_content(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }
    let end = content.floor_char_boundary(max_chars);
    content[..end].to_string()
}

/// Resolve --context-from task IDs: read output/diff from completed tasks.
pub(super) fn resolve_context_from(store: &Store, task_ids: &[String]) -> Result<Option<String>> {
    let mut blocks = Vec::new();
    for task_id in task_ids {
        if let Some(filename) = task_id.strip_prefix("shared:") {
            let Some(shared_dir) = std::env::var_os("AID_SHARED_DIR") else {
                aid_warn!("[aid] Warning: shared file '{filename}' not found, skipping");
                continue;
            };
            let path = std::path::Path::new(&shared_dir).join(filename);
            let Ok(content) = std::fs::read_to_string(&path) else {
                aid_warn!("[aid] Warning: shared file '{filename}' not found, skipping");
                continue;
            };
            let sanitized = sanitize_injected_content(&content);
            blocks.push(format!(
                "[Shared File — {filename}]\n<shared-file name=\"{filename}\">\n{}\n</shared-file>",
                sanitized.trim()
            ));
            continue;
        }
        let Some(task) = store.get_task(task_id)? else {
            aid_warn!("[aid] Warning: --context-from task '{task_id}' not found, skipping");
            continue;
        };
        let mut content = String::new();
        if let Some(ref path) = task.output_path
            && let Ok(text) = std::fs::read_to_string(path)
        {
            content = text;
        }
        if content.is_empty()
            && let Some(ref log_path) = task.log_path
        {
            if let Some(text) = extract_messages_from_log(std::path::Path::new(log_path), false) {
                content = truncate_context_content(&text, 2_000);
            } else if let Ok(text) = std::fs::read_to_string(log_path) {
                let lines: Vec<&str> = text.lines().collect();
                let start = lines.len().saturating_sub(50);
                content = lines[start..].join("\n");
            }
        }
        if content.is_empty() {
            aid_warn!("[aid] Warning: --context-from task '{task_id}' has no output, skipping");
            continue;
        }
        let sanitized = sanitize_injected_content(&content);
        blocks.push(format!(
            "[Prior Task Result — {} ({}, {})]\n<prior-task-output task=\"{}\">\n{}\n</prior-task-output>",
            task_id,
            task.agent_display_name(),
            task.status.as_str(),
            task_id,
            sanitized.trim()
        ));
    }
    if blocks.is_empty() {
        return Ok(None);
    }
    Ok(Some(blocks.join("\n\n")))
}

pub(super) fn collect_sibling_summaries(
    store: &Store,
    group_id: &str,
    current_task_id: &str,
) -> Result<Vec<CompletionSummary>> {
    let tasks = store.list_tasks_by_group(group_id)?;
    let mut summaries = Vec::new();
    for task in &tasks {
        if task.id.as_str() == current_task_id { continue; }
        if !task.status.is_terminal() { continue; }
        if let Some(json) = store.get_completion_summary(task.id.as_str())?
            && let Ok(summary) = serde_json::from_str::<CompletionSummary>(&json)
        {
            summaries.push(summary);
        }
    }
    summaries.truncate(5);
    Ok(summaries)
}

#[cfg(test)]
#[path = "prompt_context_tests.rs"]
mod tests;
