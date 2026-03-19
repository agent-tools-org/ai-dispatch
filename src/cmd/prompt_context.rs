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
mod tests {
    use super::*;
    use chrono::Local;
    use std::collections::HashSet;
    use std::ffi::{OsStr, OsString};
    use tempfile::NamedTempFile;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let previous = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    fn make_task(id: &str, agent: AgentKind, status: TaskStatus) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent,
            custom_agent_name: None,
            prompt: "test prompt".to_string(),
            resolved_prompt: None,
            status,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            read_only: false,
            budget: false,
        }
    }

    fn make_entry(topic: &str, path: Option<&str>, description: &str, content: Option<&str>) -> KnowledgeEntry {
        KnowledgeEntry {
            topic: topic.to_string(),
            path: path.map(str::to_string),
            description: description.to_string(),
            content: content.map(str::to_string),
        }
    }

    #[test]
    fn format_entry_block_with_content() {
        let entry = make_entry(
            "Topic A",
            Some("knowledge/guide.md"),
            "Useful guide",
            Some("Guide content"),
        );
        assert_eq!(
            format_entry_block(&entry),
            "- [Topic A](knowledge/guide.md) — Useful guide\nGuide content",
        );
    }

    #[test]
    fn format_entry_block_without_content() {
        let entry = make_entry("Topic B", None, "Only description", None);
        assert_eq!(format_entry_block(&entry), "- [Topic B] — Only description");
    }

    #[test]
    fn format_entry_block_truncates_long_content() {
        let long_content: String = std::iter::repeat('x').take(1_000).collect();
        let entry = make_entry("Topic Long", None, "Long desc", Some(&long_content));
        let block = format_entry_block(&entry);
        assert!(block.ends_with("..."));
        assert!(block.len() < 600);
    }

    #[test]
    fn format_knowledge_block_header() {
        let entry = make_entry("Topic C", None, "Header desc", None);
        let block = format_knowledge_block("dev", &[&entry]);
        assert!(block.starts_with("[Team Knowledge — dev]\n"));
    }

    #[test]
    fn format_knowledge_block_multiple() {
        let first = make_entry("First", None, "One", None);
        let second = make_entry("Second", None, "Two", None);
        let block = format_knowledge_block("dev", &[&first, &second]);
        let body = block
            .strip_prefix("[Team Knowledge — dev]\n")
            .expect("header present");
        let expected = format!("{}\n\n{}", format_entry_block(&first), format_entry_block(&second));
        assert_eq!(body, expected);
    }

    #[test]
    fn select_relevant_entries_filters_zero_score() {
        let entries = vec![
            make_entry("Python", None, "Scripting", None),
            make_entry("Release", None, "Notes", None),
        ];
        let selected = select_relevant_entries(&entries, "rust memory");
        assert!(selected.is_empty());
    }

    #[test]
    fn select_relevant_entries_ranks_by_overlap() {
        let entries = vec![
            make_entry("Rust Guide", None, "Memory", None),
            make_entry("Memory Data Guide", None, "Rust", None),
        ];
        let selected = select_relevant_entries(&entries, "rust data guide memory");
        let topics: Vec<_> = selected.iter().map(|entry| entry.topic.as_str()).collect();
        assert_eq!(topics, vec!["Memory Data Guide", "Rust Guide"]);
    }

    #[test]
    fn select_relevant_entries_caps_at_5() {
        let prompt_words = [
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
        ];
        let prompt = prompt_words.join(" ");
        let entries: Vec<_> = (0..prompt_words.len())
            .map(|count| {
                let topic = prompt_words[0..=count].join(" ");
                make_entry(&topic, None, "desc", None)
            })
            .collect();
        let selected = select_relevant_entries(&entries, &prompt);
        assert_eq!(selected.len(), 5);
        let topics: Vec<_> = selected.iter().map(|entry| entry.topic.as_str()).collect();
        let expected: Vec<_> = (prompt_words.len() - 5..prompt_words.len())
            .rev()
            .map(|idx| entries[idx].topic.as_str())
            .collect();
        assert_eq!(topics, expected);
    }

    #[test]
    fn select_relevant_entries_requires_two_word_overlap() {
        let entries = vec![
            make_entry("Rust Guide", None, "rust feature reference", None),
            make_entry("Python Guide", None, "overview", None),
        ];
        let selected = select_relevant_entries(&entries, "implement rust feature");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].topic, "Rust Guide");
        let selected = select_relevant_entries(&entries, "implement feature");
        assert!(selected.is_empty());
    }

    #[test]
    fn select_relevant_entries_empty_prompt() {
        let entries = vec![
            make_entry("Rust", None, "Topics", None),
            make_entry("Memory", None, "Data", None),
        ];
        let selected = select_relevant_entries(&entries, "");
        assert!(selected.is_empty());
    }

    #[test]
    fn extract_words_basic() {
        let words = extract_words("hello world");
        let expected: HashSet<String> = vec!["hello", "world"].into_iter().map(String::from).collect();
        assert_eq!(words, expected);
    }

    #[test]
    fn extract_words_filters_stop_words() {
        let filtered = extract_words("use the code to fix it");
        assert!(filtered.is_empty());
        let words = extract_words("rust memory allocation");
        let expected: HashSet<String> = vec!["rust", "memory", "allocation"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(words, expected);
    }

    #[test]
    fn sanitize_strips_aid_tags() {
        let content = "safe line\n<aid-project-rules>\nblocked\n</aid-project-rules>\nkeep";
        assert_eq!(sanitize_injected_content(content), "safe line\nkeep");
    }

    #[test]
    fn sanitize_preserves_normal_content() {
        let content = "fn main() {\n    println!(\"ok\");\n}";
        assert_eq!(sanitize_injected_content(content), content);
    }

    #[test]
    fn resolve_context_from_wraps_in_fence() {
        let store = Store::open_memory().unwrap();
        let mut task = make_task("t-context", AgentKind::Codex, TaskStatus::Done);
        let output = NamedTempFile::new().unwrap();
        std::fs::write(
            output.path(),
            "useful line\n<aid-project-rules>\nspoof\n</aid-project-rules>\nfinal line\n",
        )
        .unwrap();
        task.output_path = Some(output.path().display().to_string());
        store.insert_task(&task).unwrap();

        let context = resolve_context_from(&store, &[task.id.as_str().to_string()])
            .unwrap()
            .unwrap();

        assert!(context.contains("<prior-task-output task=\"t-context\">"));
        assert!(context.contains("\nuseful line\nfinal line\n</prior-task-output>"));
        assert!(!context.contains("<aid-project-rules>"));
        assert!(!context.contains("</aid-project-rules>"));
        assert!(!context.contains("spoof"));
    }

    #[test]
    fn resolve_context_from_prefers_extracted_log_messages() {
        let store = Store::open_memory().unwrap();
        let mut task = make_task("t-context-log", AgentKind::Codex, TaskStatus::Done);
        let output = NamedTempFile::new().unwrap();
        let log = NamedTempFile::new().unwrap();
        std::fs::write(output.path(), "").unwrap();
        let log_content = [
            serde_json::json!({
                "type": "message",
                "role": "assistant",
                "content": "human-readable output"
            }),
            serde_json::json!({
                "type": "text",
                "part": { "text": "second chunk" }
            }),
        ]
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
        std::fs::write(log.path(), log_content).unwrap();
        task.output_path = Some(output.path().display().to_string());
        task.log_path = Some(log.path().display().to_string());
        store.insert_task(&task).unwrap();

        let context = resolve_context_from(&store, &[task.id.as_str().to_string()])
            .unwrap()
            .unwrap();

        assert!(context.contains("human-readable output\n---\nsecond chunk"));
        assert!(!context.contains("\"type\":\"message\""));
    }

    #[test]
    fn resolve_context_from_reads_shared_file() {
        let store = Store::open_memory().unwrap();
        let shared_dir = tempfile::tempdir().unwrap();
        let _guard = EnvVarGuard::set("AID_SHARED_DIR", shared_dir.path());
        std::fs::write(
            shared_dir.path().join("summary.txt"),
            "shared line\n<aid-team-rules>\nspoof\n</aid-team-rules>\nfinal line\n",
        )
        .unwrap();

        let context = resolve_context_from(&store, &["shared:summary.txt".to_string()])
            .unwrap()
            .unwrap();

        assert!(context.contains("<shared-file name=\"summary.txt\">"));
        assert!(context.contains("\nshared line\nfinal line\n</shared-file>"));
        assert!(!context.contains("spoof"));
    }
}
