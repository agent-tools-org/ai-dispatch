// Summarizes workgroup tasks for the aid CLI.
// Exports: run.
// Deps: anyhow, crate::store, crate::types.
use crate::store::Store;
use crate::types::{Finding, Task, TaskStatus, Workgroup};
use anyhow::{anyhow, Result};
use std::collections::HashMap;

pub fn run(store: &Store, group_id: &str) -> Result<()> {
    let tasks = store.list_tasks_by_group(group_id)?;
    let workgroup = store
        .get_workgroup(group_id)?
        .ok_or_else(|| anyhow!("Workgroup {group_id} not found"))?;
    let milestone_map = group_milestones(store.get_workgroup_milestones(group_id)?);
    let done = tasks
        .iter()
        .filter(|task| is_success_status(task.status))
        .count();
    let failed = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Failed)
        .count();

    print_header(&workgroup, total_tasks(&tasks), done, failed);
    print_results(&tasks);
    print_milestones(&tasks, &milestone_map);
    let findings = store.list_findings(group_id)?;
    if !findings.is_empty() {
        print_findings(&findings);
    }
    Ok(())
}
fn total_tasks(tasks: &[Task]) -> usize {
    tasks.len()
}

fn print_header(workgroup: &Workgroup, total: usize, done: usize, failed: usize) {
    println!("Workgroup: {} ({})", workgroup.name, workgroup.id.as_str());
    println!("Tasks: {} total, {} done, {} failed", total, done, failed);
    println!();
}

fn print_results(tasks: &[Task]) {
    println!("Results:");
    for task in tasks {
        let symbol = status_symbol(task.status);
        let snippet = prompt_snippet(&task.prompt);
        let attrs = format_result_attrs(task);
        println!(
            "{} {} {} — \"{}\" ({})",
            symbol,
            task.id.as_str(),
            task.agent_display_name(),
            snippet,
            attrs
        );
    }
}

fn print_milestones(tasks: &[Task], milestones: &HashMap<String, Vec<String>>) {
    println!();
    println!("Milestones:");
    let mut printed = false;
    for task in tasks {
        if let Some(details) = milestones.get(task.id.as_str()) {
            println!("- {}: {}", task.id.as_str(), details.join(" → "));
            printed = true;
        }
    }
    if !printed {
        println!("- (none)");
    }
}

fn print_findings(findings: &[Finding]) {
    println!();
    println!("Findings:");
    for finding in findings {
        let source = finding.source_task_id.as_deref().unwrap_or("manual");
        println!("  [{}] {}", source, finding.content);
    }
}

fn group_milestones(entries: Vec<(String, String)>) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (task_id, detail) in entries {
        map.entry(task_id).or_default().push(detail);
    }
    map
}

fn status_symbol(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Done | TaskStatus::Merged => "✓",
        TaskStatus::Failed => "✗",
        _ => "•",
    }
}

fn is_success_status(status: TaskStatus) -> bool {
    matches!(status, TaskStatus::Done | TaskStatus::Merged)
}

fn format_result_attrs(task: &Task) -> String {
    let mut parts = Vec::new();
    if let Some(duration) = task.duration_ms {
        parts.push(format_duration(duration));
    }
    if task.status == TaskStatus::Failed {
        parts.push("FAILED".to_string());
    } else if is_success_status(task.status) {
        if let Some(tokens) = task.tokens {
            parts.push(format!("{} tokens", format_tokens(tokens)));
        }
        parts.push(format_cost_label(task.cost_usd));
    }
    if parts.is_empty() {
        parts.push("pending".to_string());
    }
    parts.join(", ")
}

fn prompt_snippet(prompt: &str) -> String {
    let compact = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    let char_count = compact.chars().count();
    if char_count <= 60 {
        compact
    } else {
        let snippet: String = compact.chars().take(60).collect();
        format!("{}…", snippet)
    }
}

fn format_duration(ms: i64) -> String {
    let secs = ms / 1_000;
    if secs >= 60 {
        let mins = secs / 60;
        let rem = secs % 60;
        if rem == 0 {
            format!("{}m", mins)
        } else {
            format!("{}m {}s", mins, rem)
        }
    } else if secs > 0 {
        format!("{}s", secs)
    } else {
        "0s".to_string()
    }
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn format_cost_label(cost: Option<f64>) -> String {
    match cost {
        Some(c) if c > 0.0 => format!("${:.2}", c),
        _ => "free".to_string(),
    }
}
