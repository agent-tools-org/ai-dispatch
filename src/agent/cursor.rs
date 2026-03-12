// Cursor Agent CLI adapter: builds `cursor agent` commands, parses text output.
// Cursor Agent runs with --trust for autonomous operation and --workspace for dir.

use anyhow::Result;
use chrono::Local;
use std::process::Command;

use super::RunOpts;
use crate::types::*;

pub struct CursorAgent;

impl super::Agent for CursorAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Cursor
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("cursor");
        cmd.args(["agent", "-p", prompt, "--trust"]);
        if let Some(ref dir) = opts.dir {
            cmd.args(["--workspace", dir]);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let now = Local::now();
        let (kind, detail) = classify_line(trimmed);
        kind.map(|k| TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: k,
            detail: truncate(detail, 80),
            metadata: None,
        })
    }

    fn parse_completion(&self, _output: &str) -> CompletionInfo {
        CompletionInfo { tokens: None, status: TaskStatus::Done, model: None, cost_usd: None }
    }
}

fn classify_line(line: &str) -> (Option<EventKind>, &str) {
    if line.contains("error[") || line.contains("FAILED") || line.starts_with("Error:") {
        (Some(EventKind::Error), line)
    } else if line.contains("test result:") || (line.contains("running") && line.contains("test")) {
        (Some(EventKind::Test), line)
    } else if line.contains("Compiling") || line.contains("Finished") {
        (Some(EventKind::Build), line)
    } else if line.contains("git commit") {
        (Some(EventKind::Commit), line)
    } else if line.starts_with("Writing") || line.starts_with("Creating") || line.contains("wrote") {
        (Some(EventKind::FileWrite), line)
    } else if line.starts_with("Reading") {
        (Some(EventKind::FileRead), line)
    } else if line.len() > 10 {
        (Some(EventKind::Reasoning), line)
    } else {
        (None, line)
    }
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.len() <= max { s } else { format!("{}...", &s[..max.saturating_sub(3)]) }
}
