// Handler for `aid reply` task message delivery and ack polling.
// Exports: run plus the reply outcome used by CLI dispatch.
// Deps: crate::store::Store, crate::input_signal, crate::types.

use std::io::Read;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};

use crate::input_signal;
use crate::store::Store;
use crate::types::{MessageDirection, MessageSource, TaskStatus};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyOutcome {
    Queued { id: i64 },
    Acked { delivered: bool },
    TimedOut { delivered: bool },
}

pub fn run(
    store: &Store,
    task_id: &str,
    message: Option<&str>,
    file: Option<&str>,
    async_mode: bool,
    timeout_secs: u64,
) -> Result<ReplyOutcome> {
    run_with_source(
        store,
        task_id,
        message,
        file,
        async_mode,
        timeout_secs,
        MessageSource::Reply,
    )
}

pub(crate) fn run_with_source(
    store: &Store,
    task_id: &str,
    message: Option<&str>,
    file: Option<&str>,
    async_mode: bool,
    timeout_secs: u64,
    source: MessageSource,
) -> Result<ReplyOutcome> {
    run_with_hook(
        store,
        task_id,
        message,
        file,
        async_mode,
        Duration::from_secs(timeout_secs),
        DEFAULT_POLL_INTERVAL,
        source,
        |_| {},
    )
}

fn run_with_hook<F>(
    store: &Store,
    task_id: &str,
    message: Option<&str>,
    file: Option<&str>,
    async_mode: bool,
    timeout: Duration,
    poll_interval: Duration,
    source: MessageSource,
    mut on_poll: F,
) -> Result<ReplyOutcome>
where
    F: FnMut(i64),
{
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("Task {task_id} not found"))?;
    if !matches!(
        task.status,
        TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled
    ) {
        bail!(
            "Task {task_id} is {} — can only reply to running tasks",
            task.status.label()
        );
    }

    let text = read_message(message, file)?;
    let queued = store.insert_message(task_id, MessageDirection::In, &text, source)?;
    input_signal::write_steer(task_id, &text)?;
    if async_mode {
        return Ok(ReplyOutcome::Queued { id: queued.id });
    }

    wait_for_ack(store, task_id, queued.id, timeout, poll_interval, &mut on_poll)
}

fn read_message(message: Option<&str>, file: Option<&str>) -> Result<String> {
    if let Some(path) = file {
        return std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read reply file: {path}"));
    }
    if let Some(message) = message {
        return Ok(message.to_string());
    }

    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("Failed to read from stdin")?;
    Ok(buf)
}

fn wait_for_ack<F>(
    store: &Store,
    task_id: &str,
    message_id: i64,
    timeout: Duration,
    poll_interval: Duration,
    on_poll: &mut F,
) -> Result<ReplyOutcome>
where
    F: FnMut(i64),
{
    let deadline = Instant::now() + timeout;
    let mut delivered = false;
    loop {
        let messages = store.list_messages_for_task(task_id)?;
        let message = messages
            .into_iter()
            .find(|entry| entry.id == message_id)
            .ok_or_else(|| anyhow!("Reply message {message_id} disappeared for task {task_id}"))?;
        delivered |= message.delivered_at.is_some();
        if message.acked_at.is_some() {
            return Ok(ReplyOutcome::Acked { delivered });
        }
        if Instant::now() >= deadline {
            return Ok(ReplyOutcome::TimedOut { delivered });
        }
        on_poll(message_id);
        std::thread::sleep(poll_interval);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::Local;

    use super::{ReplyOutcome, run, run_with_hook};
    use crate::paths::AidHomeGuard;
    use crate::store::Store;
    use crate::types::{AgentKind, MessageSource, Task, TaskId, TaskStatus, VerifyStatus};

    fn make_task(id: &str, status: TaskStatus) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "test".to_string(),
            resolved_prompt: None,
            category: None,
            status,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        }
    }

    #[test]
    fn reply_async_returns_immediately() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = AidHomeGuard::set(temp.path());
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-reply-async", TaskStatus::Running)).unwrap();

        let outcome = run(
            &store,
            "t-reply-async",
            Some("follow this path"),
            None,
            true,
            30,
        )
        .unwrap();

        assert_eq!(outcome, ReplyOutcome::Queued { id: 1 });
        let messages = store.list_messages_for_task("t-reply-async").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].source, MessageSource::Reply);
        assert_eq!(messages[0].content, "follow this path");
    }

    #[test]
    fn reply_polls_until_ack() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = AidHomeGuard::set(temp.path());
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-reply-ack", TaskStatus::AwaitingInput)).unwrap();

        let mut polls = 0usize;
        let outcome = run_with_hook(
            &store,
            "t-reply-ack",
            Some("answer"),
            None,
            false,
            Duration::from_millis(25),
            Duration::from_millis(1),
            MessageSource::Reply,
            |message_id| {
                polls += 1;
                if polls == 1 {
                    store.mark_delivered(message_id).unwrap();
                }
                if polls == 2 {
                    store.mark_acked_latest_inbound("t-reply-ack").unwrap();
                }
            },
        )
        .unwrap();

        assert_eq!(outcome, ReplyOutcome::Acked { delivered: true });
    }

    #[test]
    fn reply_times_out_cleanly() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = AidHomeGuard::set(temp.path());
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-reply-timeout", TaskStatus::Stalled)).unwrap();

        let outcome = run_with_hook(
            &store,
            "t-reply-timeout",
            Some("nudge"),
            None,
            false,
            Duration::from_millis(5),
            Duration::from_millis(1),
            MessageSource::Reply,
            |_| {},
        )
        .unwrap();

        assert_eq!(outcome, ReplyOutcome::TimedOut { delivered: false });
    }
}
