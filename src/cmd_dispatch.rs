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
use crate::types::{Task, TaskId, TaskStatus};
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
            duration_ms: task.duration_ms.or(elapsed_ms).unwrap_or(0).max(0),
            reason,
        }
    }

    pub(crate) fn exit_code(&self) -> i32 {
        exit_code_for_status(self.status)
    }

    pub(crate) fn summary_line(&self) -> String {
        let duration = format_duration(self.duration_ms);
        if self.status == TaskStatus::Done {
            return format!("[aid] {} done in {duration} (exit 0)", self.task_id);
        }
        let marker = if self.status == TaskStatus::Stopped { "stopped" } else { "failed" };
        let mut line = format!("[aid] {} {marker} in {duration} (exit 1)", self.task_id);
        if !matches!(self.status, TaskStatus::Failed | TaskStatus::Stopped) {
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
        "Task {task_id} started in background and is still running ({}: {})",
        agent_display_name,
        crate::agent::truncate::truncate_text(prompt, 50)
    )
}

fn exit_code_for_status(status: TaskStatus) -> i32 {
    if status == TaskStatus::Done { 0 } else { 1 }
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
mod tests {
    use super::{
        RunExitStatus, background_status_line, exit_code_for_status,
        resolve_finding_content_from,
    };
    use crate::types::{TaskId, TaskStatus};
    use anyhow::Result;
    use std::io::{Cursor, Write};
    use tempfile::NamedTempFile;

    #[test]
    fn resolve_finding_content_prefers_file() -> Result<()> {
        let mut file = NamedTempFile::new()?;
        write!(file, "from file")?;
        let mut stdin = Cursor::new("from stdin");

        let content = resolve_finding_content_from(
            Some("inline".to_string()),
            true,
            Some(file.path().to_string_lossy().into_owned()),
            false,
            &mut stdin,
        )?;

        assert_eq!(content, "from file");
        Ok(())
    }

    #[test]
    fn resolve_finding_content_reads_stdin_when_requested() -> Result<()> {
        let mut stdin = Cursor::new("from stdin");
        let content = resolve_finding_content_from(
            Some("inline".to_string()),
            true,
            None,
            true,
            &mut stdin,
        )?;
        assert_eq!(content, "from stdin");
        Ok(())
    }

    #[test]
    fn resolve_finding_content_errors_when_piped_without_stdin_flag() {
        let mut stdin = Cursor::new("from pipe");
        let err = resolve_finding_content_from(None, false, None, false, &mut stdin).unwrap_err();
        assert!(err.to_string().contains("No finding content provided"));
    }

    #[test]
    fn resolve_finding_content_uses_inline_arg() -> Result<()> {
        let mut stdin = Cursor::new("");
        let content = resolve_finding_content_from(
            Some("inline".to_string()),
            false,
            None,
            true,
            &mut stdin,
        )?;
        assert_eq!(content, "inline");
        Ok(())
    }

    #[test]
    fn resolve_finding_content_errors_without_input() {
        let mut stdin = Cursor::new("");
        let error = resolve_finding_content_from(None, false, None, true, &mut stdin).unwrap_err();
        assert!(error.to_string().contains("No finding content provided"));
    }

    #[test]
    fn terminal_task_statuses_map_to_process_exit_codes() {
        for status in TaskStatus::ALL.into_iter().filter(TaskStatus::is_terminal) {
            let expected = if status == TaskStatus::Done { 0 } else { 1 };
            assert_eq!(exit_code_for_status(status), expected, "status {status}");
        }
    }

    #[test]
    fn run_status_lines_are_textually_distinguishable() {
        let done = status_line(TaskStatus::Done, None);
        let failed = status_line(TaskStatus::Failed, Some("agent exited unsuccessfully"));
        let background = background_status_line(&TaskId("t-bg01".to_string()), "codex", "fix it");

        assert!(done.contains("t-test done in 2s (exit 0)"));
        assert!(!done.contains("failed"));
        assert!(!done.contains("started in background"));

        assert!(failed.contains("t-test failed in 2s (exit 1)"));
        assert!(failed.contains("agent exited unsuccessfully"));
        assert!(!failed.contains(" done "));
        assert!(!failed.contains("started in background"));

        assert!(background.contains("t-bg01 started in background"));
        assert!(background.contains("still running"));
        assert!(!background.contains("done"));
        assert!(!background.contains("finished"));
        assert!(!background.contains("failed"));
    }

    fn status_line(status: TaskStatus, reason: Option<&str>) -> String {
        RunExitStatus {
            task_id: TaskId("t-test".to_string()),
            status,
            duration_ms: 2_500,
            reason: reason.map(str::to_string),
        }
        .summary_line()
    }
}
