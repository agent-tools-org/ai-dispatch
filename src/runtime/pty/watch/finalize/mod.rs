// PTY finalize helpers moved from the shared monitor.
// Deps: parent monitor state, agent parsing, Store and PTY control.

use super::*;

pub(crate) fn finalize_output(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    output_path: Option<&str>,
    log_path: &std::path::Path,
    streaming: bool,
    exit_status: &portable_pty::ExitStatus,
    state: &mut MonitorState,
) -> Result<()> {
    append_terminal_sentinel(task_id, log_path, exit_status, state);
    if streaming {
        return finalize_streaming(agent, task_id, store, exit_status, state);
    }
    finalize_buffered(agent, task_id, store, output_path, exit_status, state)
}

pub(super) fn finalize_streaming(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    exit_status: &portable_pty::ExitStatus,
    state: &mut MonitorState,
) -> Result<()> {
    state.full_output.push_str(&terminal_sentinel(task_id, exit_status, state));
    persist_transcript(task_id, &state.full_output);
    let mut status = if state.info.status == TaskStatus::Failed {
        TaskStatus::Failed
    } else if exit_status.success() {
        TaskStatus::Done
    } else {
        TaskStatus::Failed
    };
    let delivery_outcome = state.delivery_evidence.validate();
    let delivered = if agent.kind() == AgentKind::Codex {
        matches!(&delivery_outcome, DeliveryOutcome::Delivered)
    } else {
        state.saw_completion_event
    };
    if agent.kind() == AgentKind::Codex {
        status = watcher::apply_codex_delivery_guard(
            store,
            task_id,
            status,
            delivery_outcome,
            i32::try_from(exit_status.exit_code()).ok(),
        );
    }
    if status == TaskStatus::Done {
        state.info.status = status;
        let parsed = agent.parse_completion(&state.full_output);
        crate::agent::stream_completion::merge_parsed_completion(&mut state.info, parsed);
        status = state.info.status;
    }
    // Same reason as the streaming watcher: a quota refusal exits 0 and carries
    // no error envelope, so it must be caught on the success path or not at all.
    // agy and other plain-text CLIs never echo their model, so the group a
    // quota belongs to is only knowable from what aid dispatched.
    let dispatched_model = store
        .get_task(task_id.as_str())
        .ok()
        .flatten()
        .and_then(|task| task.requested_model);
    let api_failure = state.info.status == TaskStatus::Failed
        && agent.api_route().is_some();
    if !api_failure && crate::agent::stream_completion::record_quota_exhaustion_with_delivery(
        &state.full_output,
        agent.kind(),
        agent.rate_limit_name(),
        state.info.model.as_deref().or(dispatched_model.as_deref()),
        delivered,
    )
    .should_fail()
    {
        status = TaskStatus::Failed;
    }
    state.info.status = status;
    state.info.exit_code = i32::try_from(exit_status.exit_code()).ok();
    store.insert_agent_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: if status == TaskStatus::Done {
            EventKind::Completion
        } else {
            EventKind::Error
        },
        detail: format!(
            "{} — {} events, exit code {}",
            status.label(),
            state.event_count,
            exit_status.exit_code()
        ),
        metadata: None,
    })?;
    Ok(())
}

fn finalize_buffered(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    output_path: Option<&str>,
    exit_status: &portable_pty::ExitStatus,
    state: &mut MonitorState,
) -> Result<()> {
    state.full_output.push_str(&terminal_sentinel(task_id, exit_status, state));
    persist_transcript(task_id, &state.full_output);
    if let Some(path) = output_path {
        write_output_file(agent.kind(), path, &state.full_output)?;
    }
    state.info = if state.info.status == TaskStatus::Failed {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Failed,
            model: None,
            cost_usd: None,
            exit_code: None,
        }
    } else if exit_status.success() {
        agent.parse_completion(&state.full_output)
    } else {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Failed,
            model: None,
            cost_usd: None,
            exit_code: None,
        }
    };
    state.info.exit_code = i32::try_from(exit_status.exit_code()).ok();
    let mut event = crate::agent::completion_event::make_completion_event(task_id, &state.info);
    if state.info.status == TaskStatus::Failed {
        event.event_kind = EventKind::Error;
    }
    store.insert_agent_event_with_usage(&event, &state.info)?;
    Ok(())
}

fn append_terminal_sentinel(
    task_id: &TaskId,
    log_path: &std::path::Path,
    exit_status: &portable_pty::ExitStatus,
    state: &MonitorState,
) {
    let sentinel = terminal_sentinel(task_id, exit_status, state);
    append_terminal_sentinel_line(log_path, &sentinel);
}

pub(crate) fn append_failed_terminal_sentinel(
    task_id: &TaskId,
    log_path: &std::path::Path,
    reason: &str,
) {
    let sentinel = format!("\n=== AID TASK {} FAILED ({}) ===\n", task_id, reason);
    append_terminal_sentinel_line(log_path, &sentinel);
    append_terminal_sentinel_line(&crate::support::paths::transcript_path(task_id.as_str()), &sentinel);
}

fn append_terminal_sentinel_line(log_path: &std::path::Path, sentinel: &str) {
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(log_path) else {
        return;
    };
    let _ = file.write_all(sentinel.as_bytes());
}

fn terminal_sentinel(
    task_id: &TaskId,
    exit_status: &portable_pty::ExitStatus,
    state: &MonitorState,
) -> String {
    let status = if state.info.status == TaskStatus::Failed || !exit_status.success() {
        "FAILED"
    } else {
        "DONE"
    };
    format!(
        "\n=== AID TASK {} {} (exit {}) ===\n",
        task_id,
        status,
        exit_status.exit_code()
    )
}
