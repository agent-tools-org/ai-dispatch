// Completion notification sink — JSONL append for local orchestrators.
// hiboss notifications are caller-controlled (not auto-triggered).
// Exports: notify_completion(), read_recent().

use anyhow::Result;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};

use crate::support::paths;
use crate::types::Task;

pub fn notify_completion(task: &Task) {
    let path = paths::aid_dir().join("completions.jsonl");
    let event = serde_json::json!({
        "task_id": task.id.as_str(),
        "agent": task.agent_display_name(),
        "status": task.status.label(),
        "outcome": task.outcome().as_str(),
        "verify_status": task.verify_status.as_str(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_prompt_respects_char_boundary() {
        let s = "hello world this is a test";
        assert_eq!(truncate_prompt(s, 5), "hello");
    }

    #[test]
    fn notify_completion_records_unobserved_as_unverified() {
        let _temp = crate::support::test_env::TestHome::with_dirs();
        let task = crate::types::Task {
            agent: crate::types::AgentKind::Codex,
            verify_status: crate::types::VerifyStatus::Unobserved,
            ..crate::types::Task::test_fixture(
                crate::types::TaskId("t-unobs".to_string()),
                crate::types::TaskStatus::Done,
            )
        };
        notify_completion(&task);
        let path = crate::support::paths::aid_dir().join("completions.jsonl");
        let line = std::fs::read_to_string(path).unwrap();
        let event: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
        assert_eq!(event["status"], "DONE");
        assert_eq!(event["outcome"], "unverified");
        assert_eq!(event["verify_status"], "unobserved");
        assert_ne!(event["outcome"], "delivered");
    }
}
