// Completion notification sink for external orchestrators.
// Exports JSONL append and recent-line reads via ~/.aid/completions.jsonl.
use anyhow::Result;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use crate::{paths, types::Task};

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

fn truncate_prompt(s: &str, max: usize) -> &str {
    let end = s.floor_char_boundary(max.min(s.len()));
    &s[..end]
}
