// CLI handler for `aid memory` — add, list, search, forget memories.
// Exports: add, list, search, forget.
// Deps: store::Store, types::{Memory, MemoryId, MemoryType}.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Local};
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::hash::{Hash, Hasher};

use crate::store::Store;
use crate::types::{Memory, MemoryId, MemoryType};

pub fn add(store: &Store, memory_type: &str, content: &str, project_path: Option<&str>) -> Result<()> {
    let parsed_type = parse_memory_type(memory_type)?;
    let type_label = parsed_type.label();
    let project = match project_path {
        Some(path) => Some(path.to_string()),
        None => detect_git_root()?,
    };
    let id = MemoryId::generate();
    let display_id = id.clone();
    let memory = Memory {
        id,
        memory_type: parsed_type,
        content: content.to_string(),
        source_task_id: None,
        agent: None,
        project_path: project.clone(),
        content_hash: hash_content(content),
        created_at: Local::now(),
        expires_at: None,
    };
    store.insert_memory(&memory)?;
    println!("Memory {} saved ({})", display_id, type_label);
    Ok(())
}

pub fn list(store: &Store, memory_type: Option<&str>, project_path: Option<&str>, all: bool) -> Result<()> {
    let kind = parse_optional_memory_type(memory_type)?;
    if all {
        let memories = store.list_memories(None, kind)?;
        print_memory_table(&memories);
        return Ok(());
    }
    let project = project_path.map(str::to_string).or_else(|| detect_git_root().ok().flatten());
    if project.is_none() {
        eprintln!("[aid] Not in a git repo. Use --all to list memories across all projects.");
        return Ok(());
    }
    let memories = store.list_memories(project.as_deref(), kind)?;
    print_memory_table(&memories);
    Ok(())
}

pub fn search(store: &Store, query: &str, project_path: Option<&str>) -> Result<()> {
    let project = project_path.map(str::to_string).or_else(|| detect_git_root().ok().flatten());
    if project.is_none() {
        eprintln!("[aid] Not in a git repo. Searching across all projects.");
    }
    let memories = store.search_memories(query, project.as_deref(), 20)?;
    print_memory_table(&memories);
    Ok(())
}

pub fn update(store: &Store, id: &str, content: &str) -> Result<()> {
    if store.update_memory(id, content)? {
        println!("Memory {} updated", id);
    } else {
        anyhow::bail!("Memory '{id}' not found");
    }
    Ok(())
}

pub fn forget(store: &Store, id: &str) -> Result<()> {
    store.delete_memory(id)?;
    println!("Memory {} forgotten", id);
    Ok(())
}

fn parse_optional_memory_type(value: Option<&str>) -> Result<Option<MemoryType>> {
    match value {
        Some(value) => Ok(Some(parse_memory_type(value)?)),
        None => Ok(None),
    }
}

fn parse_memory_type(value: &str) -> Result<MemoryType> {
    MemoryType::parse_str(value)
        .ok_or_else(|| anyhow!("Invalid memory type: {}", value))
}

fn hash_content(content: &str) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn detect_git_root() -> Result<Option<String>> {
    let mut dir = env::current_dir()?;
    loop {
        if dir.join(".git").exists() {
            return Ok(Some(dir.to_string_lossy().into_owned()));
        }
        if !dir.pop() {
            return Ok(None);
        }
    }
}

fn print_memory_table(memories: &[Memory]) {
    if memories.is_empty() {
        println!("No memories found.");
        return;
    }
    println!("{:<10} {:<10} {:<60} {:<8} {}", "ID", "TYPE", "CONTENT", "AGE", "SOURCE");
    println!("{}", "-".repeat(94));
    for memory in memories {
        println!(
            "{:<10} {:<10} {:<60} {:<8} {}",
            memory.id,
            memory.memory_type.label(),
            truncate(&memory.content, 60),
            format_age(&memory.created_at),
            memory.project_path.as_deref().unwrap_or("-"),
        );
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let safe = value.floor_char_boundary(max.saturating_sub(3));
    format!("{}...", &value[..safe])
}

fn format_age(created_at: &DateTime<Local>) -> String {
    let now = Local::now();
    let duration = now.signed_duration_since(*created_at);
    let mins = duration.num_minutes();
    if mins <= 0 {
        format!("{}s", duration.num_seconds().max(0))
    } else if mins < 60 {
        format!("{}m", mins)
    } else if mins < 24 * 60 {
        format!("{}h", duration.num_hours())
    } else {
        format!("{}d", duration.num_days())
    }
}
