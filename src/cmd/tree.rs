// Handler for `aid tree <task-id>` — render retry chain as ASCII tree.
// Exports: `run(&Store, &str)`; relies on store queries and task metadata.
// Deps: crate::store::Store and crate::types::{Task, TaskFilter}.
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use crate::store::Store;
use crate::types::{Task, TaskFilter};

pub fn run(store: &Store, task_id: &str) -> Result<()> {
    let chain = store.get_retry_chain(task_id)?;
    if chain.is_empty() {
        anyhow::bail!("Task '{task_id}' not found");
    }
    let chain_ids: HashSet<&str> = chain.iter().map(|task| task.id.as_str()).collect();
    let mut children_by_parent: HashMap<String, Vec<Task>> = HashMap::new();
    for task in store.list_tasks(TaskFilter::All)? {
        if let Some(parent_id) = task.parent_task_id.as_deref()
            && chain_ids.contains(parent_id)
        {
            children_by_parent.entry(parent_id.to_string()).or_default().push(task);
        }
    }
    let mut chain_child_idx: HashMap<String, usize> = HashMap::new();
    for (i, task) in chain.iter().enumerate().take(chain.len().saturating_sub(1)) {
        chain_child_idx.insert(task.id.as_str().to_string(), i + 1);
    }
    println!("{}", format_task_line(&chain[0]));
    render_children(
        &chain,
        0,
        "",
        &children_by_parent,
        &chain_child_idx,
    );
    Ok(())
}
fn render_children(
    chain: &[Task],
    parent_idx: usize,
    indent: &str,
    children_by_parent: &HashMap<String, Vec<Task>>,
    chain_child_idx: &HashMap<String, usize>,
) {
    let parent_id = chain[parent_idx].id.as_str();
    let chain_child_id = chain_child_idx
        .get(parent_id)
        .map(|&idx| &chain[idx].id);
    let mut entries: Vec<(Task, Option<usize>)> = Vec::new();
    if let Some(children) = children_by_parent.get(parent_id) {
        for child in children {
            let is_chain_child = chain_child_id == Some(&child.id);
            if !is_chain_child {
                entries.push((child.clone(), None));
            }
        }
    }
    if let Some(&child_idx) = chain_child_idx.get(parent_id) {
        entries.push((chain[child_idx].clone(), Some(child_idx)));
    }
    if entries.is_empty() {
        return;
    }
    let total = entries.len();
    for (i, (task, chain_idx)) in entries.into_iter().enumerate() {
        let is_last = i + 1 == total;
        let connector = if is_last { "└──" } else { "├──" };
        println!("{}{} {}", indent, connector, format_task_line(&task));
        if let Some(next_idx) = chain_idx {
            let child_indent = format!("{}{}", indent, if is_last { "    " } else { "│   " });
            render_children(
                chain,
                next_idx,
                &child_indent,
                children_by_parent,
                chain_child_idx,
            );
        }
    }
}
fn format_task_line(task: &Task) -> String {
    let mut status = task.status.label().to_string();
    if task.has_verify_failure() {
        status.push_str(" [verify:failed]");
    }
    if let Some(delivery) = task.delivery_assessment() {
        status.push_str(&format!(" [delivery:{}]", delivery.as_str()));
    }
    let duration = format_duration(task.duration_ms);
    let mut parts = vec![duration];
    if let Some(tokens) = task.tokens {
        parts.push(format!("{} tokens", format_token_count(tokens)));
    }
    format!(
        "{} {} {} ({})",
        task.id.as_str(),
        task.agent_display_name(),
        status,
        parts.join(", ")
    )
}
fn format_duration(duration_ms: Option<i64>) -> String {
    if let Some(ms) = duration_ms {
        let secs = ms / 1000;
        if secs < 60 {
            format!("{}s", secs)
        } else {
            format!("{}m {:02}s", secs / 60, secs % 60)
        }
    } else {
        "n/a".to_string()
    }
}
fn format_token_count(count: i64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}
