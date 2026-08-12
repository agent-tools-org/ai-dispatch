// aid CLI command dispatch support.
// Exports shared dispatch helpers and finding-content resolution logic.

mod admin_config;
mod dispatch_match;
mod display;
mod knowledge;
mod project_worktree;
mod recommend_hint;
mod run_batch;
mod task_ops;

use crate::cli::Commands;
use crate::store::Store;
use crate::types::{Task, TaskId, TaskOutcome, TaskStatus};
use anyhow::{Result, anyhow, bail};
use std::fs;
use std::io::{IsTerminal, Read};
use std::sync::Arc;

pub(crate) async fn dispatch(store: Arc<Store>, command: Commands) -> Result<DispatchOutcome> {
    dispatch_match::dispatch(store, command).await
}

pub(crate) enum DispatchOutcome {
    CommandCompleted,
    Run(RunDispatch),
}

pub(crate) struct RunDispatch {
    task_id: TaskId,
    background: bool,
    dry_run: bool,
}

impl RunDispatch {
    pub(crate) fn new(task_id: TaskId, background: bool, dry_run: bool) -> Self {
        Self { task_id, background, dry_run }
    }
}

impl DispatchOutcome {
    pub(crate) fn run_exit_status(self, store: &Store) -> Result<Option<RunExitStatus>> {
        let Self::Run(run) = self else {
            return Ok(None);
        };
        if run.background || run.dry_run {
            return Ok(None);
        }
        let task = store
            .get_task(run.task_id.as_str())?
            .ok_or_else(|| anyhow!("Task '{}' not found after foreground run", run.task_id))?;
        Ok(Some(RunExitStatus::from_task(&task, store.latest_error(task.id.as_str()))))
    }
}

pub(crate) struct RunExitStatus {
    task_id: TaskId,
    status: TaskStatus,
    outcome: TaskOutcome,
    duration_ms: i64,
    reason: Option<String>,
}

impl RunExitStatus {
    fn from_task(task: &Task, reason: Option<String>) -> Self {
        let elapsed_ms = task.completed_at
            .map(|completed| (completed - task.created_at).num_milliseconds());
        Self {
            task_id: task.id.clone(),
            status: task.status,
            outcome: task.outcome(),
            duration_ms: task.duration_ms.or(elapsed_ms).unwrap_or(0).max(0),
            reason,
        }
    }

    pub(crate) fn exit_code(&self) -> i32 {
        exit_code_for_outcome(self.outcome)
    }

    pub(crate) fn summary_line(&self) -> String {
        let duration = format_duration(self.duration_ms);
        let mut line = match self.outcome {
            TaskOutcome::Verified | TaskOutcome::Delivered => {
                return format!("[STATUS=DONE] [aid] {} done in {duration} (exit 0)", self.task_id)
            }
            TaskOutcome::Broken => format!(
                "[STATUS=VERIFY_FAILED] [aid] {} completed but verification failed in {duration} (exit 1)",
                self.task_id
            ),
            TaskOutcome::Unverified(reason) => format!(
                "[STATUS=UNVERIFIED] [aid] {} completed but verification was inconclusive ({reason:?}) in {duration} (exit 1)",
                self.task_id
            ),
            _ => {
                let marker = if matches!(self.outcome, TaskOutcome::Stopped) { "stopped" } else { "failed" };
                format!("[STATUS=FAILED] [aid] {} {marker} in {duration} (exit 1)", self.task_id)
            }
        };
        if matches!(self.outcome, TaskOutcome::InProgress | TaskOutcome::Skipped) {
            line.push_str(&format!(" — status {}", self.status));
        }
        if let Some(reason) = self.reason.as_deref().filter(|reason| !reason.trim().is_empty()) {
            let concise = reason.split_whitespace().collect::<Vec<_>>().join(" ");
            line.push_str(&format!(" — {}", crate::agent::truncate::truncate_text(&concise, 120)));
        }
        line
    }
}

pub(crate) fn background_status_line(
    task_id: &TaskId,
    agent_display_name: &str,
    prompt: &str,
) -> String {
    format!(
        "[STATUS=BG_RUNNING] Task {task_id} started in background and is still running ({}: {})",
        agent_display_name,
        crate::agent::truncate::truncate_text(prompt, 50)
    )
}

fn exit_code_for_outcome(outcome: TaskOutcome) -> i32 {
    if outcome.is_success() { 0 } else { 1 }
}

fn format_duration(duration_ms: i64) -> String {
    let seconds = duration_ms / 1_000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m {:02}s", seconds / 3_600, (seconds % 3_600) / 60, seconds % 60)
    }
}

pub(crate) fn resolve_group(flag: Option<String>) -> Option<String> {
    flag.or_else(|| std::env::var("AID_GROUP").ok())
}

pub(crate) fn resolve_finding_content(
    content: Option<String>,
    stdin: bool,
    file: Option<String>,
) -> Result<String> {
    let stdin_is_terminal = std::io::stdin().is_terminal();
    resolve_finding_content_from(content, stdin, file, stdin_is_terminal, &mut std::io::stdin())
}

pub(crate) fn resolve_finding_content_from<R: Read>(
    content: Option<String>,
    stdin: bool,
    file: Option<String>,
    _stdin_is_terminal: bool,
    reader: &mut R,
) -> Result<String> {
    if let Some(path) = file {
        return Ok(fs::read_to_string(path)?);
    }
    // Only read stdin when --stdin is explicitly passed (#101).
    // Previously this also auto-read when stdin was not a terminal,
    // but in background tasks stdin is /dev/null, causing empty reads.
    if stdin {
        let mut buffer = String::new();
        reader.read_to_string(&mut buffer)?;
        return Ok(buffer);
    }
    if let Some(content) = content {
        return Ok(content);
    }
    bail!("No finding content provided. Pass content as an argument, --file <path>, or --stdin")
}

#[cfg(test)]
#[path = "cmd_dispatch/tests.rs"]
mod tests;
