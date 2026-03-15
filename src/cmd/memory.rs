// CLI handler for `aid memory` — add, list, search, forget memories.
// Exports: add, list, search, forget.
// Deps: store::Store, types::{Memory, MemoryId, MemoryType}.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Local};
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::hash::{Hash, Hasher};

use crate::config;
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
        supersedes: None,
        version: 1,
        inject_count: 0,
        last_injected_at: None,
        success_count: 0,
    };
    store.insert_memory(&memory)?;
    println!("Memory {} saved ({})", display_id, type_label);
    Ok(())
}

pub fn list(store: &Store, memory_type: Option<&str>, project_path: Option<&str>, all: bool, stats: bool) -> Result<()> {
    let kind = parse_optional_memory_type(memory_type)?;
    if all {
        let memories = store.list_memories(None, kind)?;
        print_memory_table(&memories, stats);
        return Ok(());
    }
    let project = project_path.map(str::to_string).or_else(|| detect_git_root().ok().flatten());
    if project.is_none() {
        eprintln!("[aid] Not in a git repo. Use --all to list memories across all projects.");
        return Ok(());
    }
    let memories = store.list_memories(project.as_deref(), kind)?;
    print_memory_table(&memories, stats);
    Ok(())
}

pub fn search(store: &Store, query: &str, project_path: Option<&str>) -> Result<()> {
    let project = project_path.map(str::to_string).or_else(|| detect_git_root().ok().flatten());
    if project.is_none() {
        eprintln!("[aid] Not in a git repo. Searching across all projects.");
    }
    let memories = store.search_memories(query, project.as_deref(), 20)?;
    print_memory_table(&memories, false);
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

pub fn history(store: &Store, id: &str) -> Result<()> {
    let chain = store.memory_history(id)?;
    if chain.is_empty() {
        println!("Memory {} not found.", id);
        return Ok(());
    }
    let entries: Vec<String> = chain
        .iter()
        .map(|memory| {
            format!(
                "v{} ({}) {}",
                memory.version,
                format_age(&memory.created_at),
                truncate(&memory.content, 60),
            )
        })
        .collect();
    println!("{}", entries.join(" -> "));
    Ok(())
}

pub fn cloud_status() -> Result<()> {
    let config = config::load_config()?;
    if !config.evermemos.enabled {
        println!("EverMemOS: disabled");
        println!("Enable in ~/.aid/config.toml:");
        println!("  [evermemos]");
        println!("  enabled = true");
        println!("  base_url = \"http://localhost:1995/api/v1\"");
        return Ok(());
    }
    let client = crate::evermemos::EverMemosClient::from_config(&config.evermemos)
        .ok_or_else(|| anyhow!("EverMemOS not configured"))?;
    match client.health_check() {
        Ok(true) => println!("EverMemOS: connected ({})", config.evermemos.base_url),
        Ok(false) => println!("EverMemOS: unhealthy ({})", config.evermemos.base_url),
        Err(e) => println!("EverMemOS: error — {e}"),
    }
    Ok(())
}

pub fn cloud_search(query: &str, limit: usize) -> Result<()> {
    let config = config::load_config()?;
    let client = crate::evermemos::EverMemosClient::from_config(&config.evermemos)
        .ok_or_else(|| anyhow!("EverMemOS not enabled. Set [evermemos] enabled=true in ~/.aid/config.toml"))?;
    let memories = client.search_memories(query, limit)?;
    if memories.is_empty() {
        println!("No cloud memories found.");
        return Ok(());
    }
    println!("{:<8} {}", "SCORE", "CONTENT");
    println!("{}", "-".repeat(80));
    for mem in &memories {
        println!("{:<8.2} {}", mem.score, truncate(&mem.content, 70));
    }
    Ok(())
}

pub fn cloud_push(store: &Store, memory_type: Option<&str>) -> Result<()> {
    let config = config::load_config()?;
    let client = crate::evermemos::EverMemosClient::from_config(&config.evermemos)
        .ok_or_else(|| anyhow!("EverMemOS not enabled"))?;
    let kind = parse_optional_memory_type(memory_type)?;
    let memories = store.list_memories(None, kind)?;
    let mut pushed = 0;
    for memory in &memories {
        let metadata = crate::evermemos::MemoryMetadata {
            task_id: memory.source_task_id.clone(),
            agent: memory.agent.clone(),
            project: memory.project_path.clone(),
            memory_type: memory.memory_type.label().to_string(),
        };
        if let Err(e) = client.store_memory(&memory.content, &metadata) {
            eprintln!("[aid] Failed to push {}: {e}", memory.id);
        } else {
            pushed += 1;
        }
    }
    println!("Pushed {}/{} memories to EverMemOS cloud", pushed, memories.len());
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

fn print_memory_table(memories: &[Memory], stats: bool) {
    if memories.is_empty() {
        println!("No memories found.");
        return;
    }
    if stats {
        println!(
            "{:<10} {:<10} {:<8} {:<9} {:<19} {}",
            "ID", "TYPE", "INJECTS", "SUCCESS", "LAST USED", "CONTENT"
        );
        println!("{}", "-".repeat(120));
        for memory in memories {
            println!(
                "{:<10} {:<10} {:<8} {:<9} {:<19} {}",
                memory.id,
                memory.memory_type.label(),
                memory.inject_count,
                memory.success_count,
                format_last_used(memory.last_injected_at.as_ref()),
                truncate(&memory.content, 60),
            );
        }
    } else {
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
}

fn format_last_used(last: Option<&DateTime<Local>>) -> String {
    last.map(|dt| format_age(dt)).unwrap_or_else(|| "-".to_string())
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
