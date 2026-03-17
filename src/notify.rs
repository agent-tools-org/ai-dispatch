// Completion notification sink for external orchestrators.
// Exports JSONL append, recent-line reads, and hiboss integration.
// Deps: config, paths, types, store (for workgroup queries).

use anyhow::Result;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use crate::config;
use crate::paths;
use crate::store::Store;
use crate::types::{Task, TaskStatus};

pub fn notify_completion(task: &Task) {
    let path = paths::aid_dir().join("completions.jsonl");
    let event = serde_json::json!({
        "task_id": task.id.as_str(),
        "agent": task.agent_display_name(),
        "status": task.status.label(),
        "duration_ms": task.duration_ms,
        "cost_usd": task.cost_usd,
        "prompt": truncate_prompt(&task.prompt, 100),
        "timestamp": chrono::Local::now().to_rfc3339(),
    });
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{event}");
    }
    notify_hiboss(task);
}

pub fn read_recent(limit: usize) -> Result<String> {
    let path = paths::aid_dir().join("completions.jsonl");
    if !path.exists() {
        return Ok(String::new());
    }
    let lines = BufReader::new(std::fs::File::open(path)?)
        .lines()
        .collect::<std::io::Result<Vec<_>>>()?;
    Ok(lines
        .into_iter()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Check if all tasks in a workgroup have completed, and send a summary notification.
pub fn notify_workgroup_if_complete(store: &Store, workgroup_id: &str) {
    let tasks = match store.list_tasks_by_group(workgroup_id) {
        Ok(tasks) => tasks,
        Err(_) => return,
    };
    if tasks.is_empty() || !tasks.iter().all(|t| t.status.is_terminal()) {
        return;
    }
    let config = match config::load_config() {
        Ok(cfg) if cfg.hiboss.enabled => cfg,
        _ => return,
    };
    let total = tasks.len();
    let done = tasks.iter().filter(|t| t.status == TaskStatus::Done || t.status == TaskStatus::Merged).count();
    let failed = tasks.iter().filter(|t| t.status == TaskStatus::Failed).count();
    let total_cost: f64 = tasks.iter().filter_map(|t| t.cost_usd).sum();
    let total_duration_ms: i64 = tasks.iter().filter_map(|t| t.duration_ms).sum();
    let duration = format_duration(Some(total_duration_ms));
    let status_line = if failed == 0 {
        format!("{done}/{total} done")
    } else {
        format!("{done}/{total} done, {failed} failed")
    };
    let message = format!(
        "Workgroup {workgroup_id}: {status_line}, {duration}, ${total_cost:.2} total"
    );
    let priority = if failed > 0 { "high" } else { &config.hiboss.priority };
    spawn_hiboss(&["send", "--priority", priority, "--type", "aid_wg_complete", &message]);
}

/// Send a budget threshold alert via hiboss.
/// Called externally when usage tracking detects budget crossing 80%.
#[allow(dead_code)]
pub fn notify_budget_alert(current_usd: f64, limit_usd: f64) {
    let config = match config::load_config() {
        Ok(cfg) if cfg.hiboss.enabled => cfg,
        _ => return,
    };
    let pct = (current_usd / limit_usd * 100.0) as u32;
    if pct < 80 {
        return;
    }
    let _ = config; // config loaded only for enabled check
    let message = format!(
        "Budget {pct}% consumed: ${current_usd:.2}/${limit_usd:.2}"
    );
    spawn_hiboss(&["send", "--priority", "critical", "--type", "aid_budget_alert", &message]);
}

fn notify_hiboss(task: &Task) {
    let config = match config::load_config() {
        Ok(cfg) if cfg.hiboss.enabled => cfg,
        _ => return,
    };
    let prompt = truncate_prompt(&task.prompt, 120);
    let duration = format_duration(task.duration_ms);
    let cost = format_cost(task.cost_usd);
    let template = config
        .hiboss
        .template
        .as_deref()
        .unwrap_or("Task {id} {status}: {prompt_truncated} ({duration}, {cost})");
    let message = template
        .replace("{id}", task.id.as_str())
        .replace("{status}", task.status.label())
        .replace("{prompt_truncated}", prompt)
        .replace("{duration}", &duration)
        .replace("{cost}", &cost);
    let (priority, msg_type) = match task.status {
        TaskStatus::Done | TaskStatus::Merged => ("normal", "aid_task_complete"),
        TaskStatus::Failed => ("high", "aid_task_fail"),
        TaskStatus::Stopped => ("normal", "aid_task_stopped"),
        _ => ("normal", "aid_task_update"),
    };
    spawn_hiboss(&["send", "--priority", priority, "--type", msg_type, &message]);
}

fn spawn_hiboss(args: &[&str]) {
    let mut cmd = Command::new("hiboss");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Err(err) = cmd.spawn()
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("hiboss send failed: {err}");
    }
}

fn format_duration(duration_ms: Option<i64>) -> String {
    if let Some(ms) = duration_ms {
        let secs = (ms / 1000).max(0);
        let mins = secs / 60;
        let secs = secs % 60;
        return if mins > 0 {
            format!("{mins}m {secs}s")
        } else {
            format!("{secs}s")
        };
    }
    "unknown duration".to_string()
}

fn format_cost(cost: Option<f64>) -> String {
    match cost {
        Some(c) => format!("${:.2}", c),
        None => "no cost data".to_string(),
    }
}

fn truncate_prompt(s: &str, max: usize) -> &str {
    let end = s.floor_char_boundary(max.min(s.len()));
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_minutes_and_seconds() {
        assert_eq!(format_duration(Some(125_000)), "2m 5s");
    }

    #[test]
    fn format_duration_seconds_only() {
        assert_eq!(format_duration(Some(45_000)), "45s");
    }

    #[test]
    fn format_cost_formats_two_decimals() {
        assert_eq!(format_cost(Some(3.5)), "$3.50");
    }

    #[test]
    fn truncate_prompt_respects_char_boundary() {
        let s = "hello world this is a test";
        assert_eq!(truncate_prompt(s, 5), "hello");
    }

    #[test]
    fn budget_alert_skips_below_threshold() {
        // Should not panic — just a no-op when hiboss is disabled
        notify_budget_alert(70.0, 100.0);
    }
}
