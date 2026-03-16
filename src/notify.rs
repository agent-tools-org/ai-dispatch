// Completion notification sink for external orchestrators.
// Exports JSONL append and recent-line reads via ~/.aid/completions.jsonl.
use anyhow::Result;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use crate::{config, paths, types::Task};

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
    let mut cmd = Command::new("hiboss");
    cmd.arg("send").arg("-p").arg(&config.hiboss.priority).arg(&message)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Err(err) = cmd.spawn()
        && err.kind() != std::io::ErrorKind::NotFound {
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
