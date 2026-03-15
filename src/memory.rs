// Post-task memory extraction from agent output.
// Parses [MEMORY: type] tags and saves to store.
// Exports: extract_and_save_memories.
// Deps: store::Store, types::{Memory, MemoryId, MemoryType}.

use anyhow::Result;
use chrono::{Duration, Local};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::store::Store;
use crate::types::{Memory, MemoryId, MemoryType};

/// Parse [MEMORY: type] content lines from agent output text.
pub fn parse_memory_tags(text: &str) -> Vec<(MemoryType, String)> {
    let mut results = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("[MEMORY:") {
            if let Some(close) = rest.find(']') {
                let type_str = rest[..close].trim();
                let content = rest[close + 1..].trim();
                if !content.is_empty() {
                    if let Some(mem_type) = MemoryType::parse_str(type_str) {
                        results.push((mem_type, content.to_string()));
                    }
                }
            }
        }
    }
    results
}

/// Compute a content hash for deduplication.
fn content_hash(content: &str) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Extract memories from agent output and save to store.
pub fn extract_and_save_memories(
    store: &Store,
    output: &str,
    task_id: &str,
    agent: Option<&str>,
    project_path: Option<&str>,
) -> Result<usize> {
    let tags = parse_memory_tags(output);
    let mut saved = 0;
    for (mem_type, content) in &tags {
        let hash = content_hash(content);
        let expires_at = if *mem_type == MemoryType::Lesson {
            Some(Local::now() + Duration::days(30))
        } else {
            None
        };
        let memory = Memory {
            id: MemoryId::generate(),
            memory_type: *mem_type,
            content: content.clone(),
            source_task_id: Some(task_id.to_string()),
            agent: agent.map(|s| s.to_string()),
            project_path: project_path.map(|s| s.to_string()),
            content_hash: hash,
            created_at: Local::now(),
            expires_at,
        };
        store.insert_memory(&memory)?;
        saved += 1;
    }
    if saved > 0 {
        eprintln!("[aid] Extracted {saved} memories from task {task_id}");
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MemoryType;

    #[test]
    fn parses_single_memory_tag() {
        let text = "[MEMORY: discovery] Auth uses bcrypt";
        let tags = parse_memory_tags(text);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0], (MemoryType::Discovery, "Auth uses bcrypt".to_string()));
    }

    #[test]
    fn parses_multiple_memory_tags() {
        let text = "[MEMORY: discovery] First line\n[MEMORY: lesson] Second line";
        let tags = parse_memory_tags(text);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].0, MemoryType::Discovery);
        assert_eq!(tags[1].0, MemoryType::Lesson);
    }

    #[test]
    fn skips_invalid_type() {
        let text = "[MEMORY: unknown] Bad";
        let tags = parse_memory_tags(text);
        assert!(tags.is_empty());
    }

    #[test]
    fn skips_empty_content() {
        let text = "[MEMORY: discovery]    ";
        let tags = parse_memory_tags(text);
        assert!(tags.is_empty());
    }
}
