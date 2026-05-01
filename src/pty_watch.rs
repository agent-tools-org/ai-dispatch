// PTY monitoring helpers for interactive background tasks.
// Owns chunk parsing, prompt detection, input forwarding, and completion finalization.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::io::Write;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::agent::Agent;
use crate::cmd::run_hung_recovery;
use crate::input_signal;
use crate::prompt::PromptDetector;
use crate::pty_bridge::PtyBridge;
use crate::pty_watch_idle::{IdleAction, IdleDetector, MonitorTaskStatus};
use crate::store::Store;
use crate::types::{CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};
use crate::watcher::{self, SyntheticMilestoneTracker};

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) struct MonitorState {
    pub(crate) info: CompletionInfo,
    full_output: String,
    line_buffer: String,
    event_count: u32,
    last_event_detail: Option<String>,
    synthetic_tracker: SyntheticMilestoneTracker,
    prompt_detector: PromptDetector,
    awaiting_input: bool,
    last_output_time: Instant,
    idle_nudged: bool,
    idle_warned: bool,
    pending_inbound_acks: usize,
    idle_detector: IdleDetector,
    streaming: bool,
}

impl MonitorState {
    pub(crate) fn new(streaming: bool) -> Self {
        Self {
            info: CompletionInfo {
                tokens: None,
                status: TaskStatus::Done,
                model: None,
                cost_usd: None,
                exit_code: None,
            },
            full_output: String::new(),
            line_buffer: String::new(),
            event_count: 0,
            last_event_detail: None,
            synthetic_tracker: SyntheticMilestoneTracker::new(),
            prompt_detector: PromptDetector::default(),
            awaiting_input: false,
            last_output_time: Instant::now(),
            idle_nudged: false,
            idle_warned: false,
            pending_inbound_acks: 0,
            idle_detector: IdleDetector::load(),
            streaming,
        }
    }

    fn handle_chunk(
        &mut self,
        agent: &dyn Agent,
        task_id: &TaskId,
        store: &Arc<Store>,
        log_file: &mut std::fs::File,
        chunk: String,
    ) -> Result<()> {
        log_file.write_all(chunk.as_bytes())?;
        self.full_output.push_str(&chunk);
        self.line_buffer.push_str(&chunk);
        self.flush_output_lines(agent, task_id, store)?;
        if !self.streaming
            && let Some(prompt) = self.prompt_detector.push_chunk(&chunk, Instant::now())
        {
            let awaiting_prompt = extract_awaiting_prompt(&self.full_output, &prompt);
            mark_awaiting_input(
                store,
                task_id,
                &prompt,
                &awaiting_prompt,
                &mut self.awaiting_input,
            )?;
        }
        Ok(())
    }

    fn flush_output_lines(
        &mut self,
        agent: &dyn Agent,
        task_id: &TaskId,
        store: &Arc<Store>,
    ) -> Result<()> {
        while let Some(pos) = self.line_buffer.find('\n') {
            let line = self.line_buffer[..pos].trim_end_matches('\r').to_string();
            self.observe_output_line(task_id, store, &line)?;
            if self.streaming {
                if let Some(event_detail) = watcher::handle_streaming_line_with_session(
                    watcher::StreamLineContext {
                        agent,
                        task_id,
                        store,
                        workgroup_id: None,
                        synthetic_tracker: &mut self.synthetic_tracker,
                    },
                    &mut self.info,
                    &mut self.event_count,
                    &line,
                    &mut false,
                )? {
                    self.last_event_detail = Some(event_detail.detail);
                } else if !line.trim().is_empty() {
                    self.last_event_detail = Some(line.trim().to_string());
                }
            } else if !line.trim().is_empty() {
                self.last_event_detail = Some(line.trim().to_string());
            }
            self.line_buffer.drain(..=pos);
        }
        Ok(())
    }

    fn flush_trailing_output(
        &mut self,
        agent: &dyn Agent,
        task_id: &TaskId,
        store: &Arc<Store>,
    ) -> Result<()> {
        let trailing = self.line_buffer.trim_end_matches(['\r', '\n']).to_string();
        if trailing.trim().is_empty() {
            return Ok(());
        }
        self.observe_output_line(task_id, store, &trailing)?;
        if self.streaming {
            watcher::handle_streaming_line(
                agent,
                task_id,
                store,
                &mut self.info,
                &mut self.event_count,
                &mut self.synthetic_tracker,
                None,
                &trailing,
            )?;
        }
        self.last_event_detail = Some(trailing.trim().to_string());
        self.line_buffer.clear();
        Ok(())
    }

    fn observe_output_line(
        &mut self,
        task_id: &TaskId,
        store: &Arc<Store>,
        line: &str,
    ) -> Result<()> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        self.last_output_time = Instant::now();
        self.idle_warned = false;
        self.idle_nudged = false;
        if self.pending_inbound_acks == 0 {
            return Ok(());
        }
        if store.mark_acked_latest_inbound(task_id.as_str())? {
            self.pending_inbound_acks -= 1;
            store.insert_event(&TaskEvent {
                task_id: task_id.clone(),
                timestamp: Local::now(),
                event_kind: EventKind::Reasoning,
                detail: "Acked reply".to_string(),
                metadata: Some(json!({ "acked_reply": true })),
            })?;
        } else {
            self.pending_inbound_acks = 0;
        }
        Ok(())
    }

    fn handle_timeout(&mut self, store: &Arc<Store>, task_id: &TaskId) -> Result<()> {
        if !self.streaming
            && let Some(prompt) = self.prompt_detector.poll_idle(Instant::now())
        {
            let awaiting_prompt = extract_awaiting_prompt(&self.full_output, &prompt);
            mark_awaiting_input(
                store,
                task_id,
                &prompt,
                &awaiting_prompt,
                &mut self.awaiting_input,
            )?;
        }
        Ok(())
    }

    fn maybe_forward_input(
        &mut self,
        bridge: &mut PtyBridge,
        store: &Arc<Store>,
        task_id: &TaskId,
    ) -> Result<()> {
        if !self.awaiting_input {
            return Ok(());
        }
        let Some(input) = input_signal::take_response(task_id.as_str())? else {
            return Ok(());
        };
        bridge.write_input(&input)?;
        self.finish_input_delivery(store, task_id)?;
        Ok(())
    }

    fn maybe_forward_steer(
        &mut self,
        bridge: &mut PtyBridge,
        store: &Arc<Store>,
        task_id: &TaskId,
    ) -> Result<()> {
        let Some(message) = input_signal::take_steer(task_id.as_str())? else {
            return Ok(());
        };
        bridge.write_input(&message)?;
        let delivered = store.mark_delivered_matching_inbound(task_id.as_str(), &message)?;
        if delivered {
            self.pending_inbound_acks += 1;
        }
        self.finish_input_delivery(store, task_id)?;
        store.insert_event(&TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: format!("Steered: {}", message.chars().take(200).collect::<String>()),
            metadata: Some(json!({ "steered": true, "delivered": delivered })),
        })?;
        Ok(())
    }

    fn maybe_consume_reply(
        &mut self,
        bridge: &mut PtyBridge,
        store: &Arc<Store>,
        task_id: &TaskId,
    ) -> Result<()> {
        for message in store.pending_inbound_for_task(task_id.as_str())? {
            bridge.write_input(&message.content)?;
            if store.mark_delivered(message.id)? {
                self.pending_inbound_acks += 1;
            }
            self.finish_input_delivery(store, task_id)?;
            store.insert_event(&TaskEvent {
                task_id: task_id.clone(),
                timestamp: Local::now(),
                event_kind: EventKind::Reasoning,
                detail: format!(
                    "Replied: {}",
                    message.content.chars().take(200).collect::<String>()
                ),
                metadata: Some(json!({
                    "message_id": message.id,
                    "source": message.source,
                })),
            })?;
        }
        Ok(())
    }

    fn maybe_handle_idle(&mut self, store: &Arc<Store>, task_id: &TaskId) -> Result<()> {
        match self.idle_detector.tick(
            self.last_output_time,
            load_monitor_status(store.as_ref(), task_id.as_str())?,
            self.idle_nudged,
        ) {
            IdleAction::None => {}
            IdleAction::WarnEvent if !self.idle_warned => {
                self.idle_warned = true;
                store.insert_event(&TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Reasoning,
                    detail: "idle warn".to_string(),
                    metadata: Some(json!({ "idle_warn": true })),
                })?;
            }
            IdleAction::WarnEvent => {}
            IdleAction::SendNudge(message) => {
                crate::unstick::queue_auto_nudge(store.as_ref(), task_id.as_str(), &message)?;
                self.idle_nudged = true;
                self.idle_warned = true;
                store.insert_event(&TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Reasoning,
                    detail: "Auto-nudge sent".to_string(),
                    metadata: Some(json!({ "message": message, "source": "unstick-auto" })),
                })?;
            }
            IdleAction::Escalate if crate::unstick::mark_task_stalled(store.as_ref(), task_id.as_str())? => {
                store.insert_event(&TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Milestone,
                    detail: "Auto-escalated: task stalled".to_string(),
                    metadata: Some(json!({ "auto_escalated": true })),
                })?;
            }
            IdleAction::Escalate => {}
        }
        Ok(())
    }

    fn finish_input_delivery(&mut self, store: &Arc<Store>, task_id: &TaskId) -> Result<()> {
        if !self.awaiting_input {
            return Ok(());
        }
        store.update_task_status(task_id.as_str(), TaskStatus::Running)?;
        self.awaiting_input = false;
        self.prompt_detector.reset_after_input();
        Ok(())
    }

    fn progress_count(&self) -> u32 {
        self.event_count.max(
            self.full_output
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count() as u32,
        )
    }

    fn last_progress_detail(&self) -> Option<String> {
        self.last_event_detail.clone().or_else(|| {
            self.full_output
                .lines()
                .rev()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(str::to_string)
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn monitor_bridge(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    bridge: &mut PtyBridge,
    rx: &mpsc::Receiver<Vec<u8>>,
    log_file: &mut std::fs::File,
    state: &mut MonitorState,
    _streaming: bool,
    idle_timeout: Option<Duration>,
    deadline: Option<Instant>,
) -> Result<()> {
    let mut reader_done = false;
    let mut child_exited_at: Option<Instant> = None;
    const CHILD_EXIT_DRAIN: Duration = Duration::from_secs(2);
    loop {
        if reader_done && !bridge.is_alive() {
            break;
        }
        if !reader_done && !bridge.is_alive() {
            let exited_at = *child_exited_at.get_or_insert_with(Instant::now);
            if exited_at.elapsed() > CHILD_EXIT_DRAIN {
                break;
            }
        }
        match rx.recv_timeout(INPUT_POLL_INTERVAL) {
            Ok(bytes) => {
                let chunk = String::from_utf8_lossy(&bytes).into_owned();
                state.handle_chunk(agent, task_id, store, log_file, chunk)?;
            }
            Err(RecvTimeoutError::Timeout) => {
                state.handle_timeout(store, task_id)?;
                if let Some(dl) = deadline
                    && Instant::now() > dl
                {
                    state.info.status = TaskStatus::Failed;
                    store.insert_event(&TaskEvent {
                        task_id: task_id.clone(),
                        timestamp: chrono::Local::now(),
                        event_kind: EventKind::Error,
                        detail: "Task exceeded deadline".to_string(),
                        metadata: None,
                    })?;
                    break;
                }
                if let Some(idle) = idle_timeout
                    && state.last_output_time.elapsed() > idle
                {
                    state.info.status = TaskStatus::Failed;
                    run_hung_recovery::insert_hung_detected_events(
                        store.as_ref(),
                        task_id,
                        idle.as_secs(),
                        state.progress_count(),
                        state.last_progress_detail().as_deref(),
                    )?;
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => reader_done = true,
        }
        state.maybe_forward_input(bridge, store, task_id)?;
        state.maybe_forward_steer(bridge, store, task_id)?;
        state.maybe_consume_reply(bridge, store, task_id)?;
        state.maybe_handle_idle(store, task_id)?;
    }

    if !state.line_buffer.trim().is_empty() {
        state.flush_trailing_output(agent, task_id, store)?;
    }
    Ok(())
}

pub(crate) fn finalize_output(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    output_path: Option<&str>,
    streaming: bool,
    exit_status: &portable_pty::ExitStatus,
    state: &mut MonitorState,
) -> Result<()> {
    if streaming {
        finalize_streaming(task_id, store, exit_status, state)
    } else {
        finalize_buffered(agent, task_id, store, output_path, exit_status, state)
    }
}

fn finalize_streaming(
    task_id: &TaskId,
    store: &Arc<Store>,
    exit_status: &portable_pty::ExitStatus,
    state: &mut MonitorState,
) -> Result<()> {
    persist_transcript(task_id, &state.full_output);
    let status = if exit_status.success() {
        TaskStatus::Done
    } else {
        TaskStatus::Failed
    };
    state.info.status = status;
    state.info.exit_code = i32::try_from(exit_status.exit_code()).ok();
    store.insert_event(&TaskEvent {
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
    persist_transcript(task_id, &state.full_output);
    if let Some(path) = output_path {
        write_output_file(path, &state.full_output)?;
    }
    state.info = if exit_status.success() {
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
    store.insert_event(&crate::agent::gemini::make_completion_event(
        task_id,
        &state.info,
    ))?;
    Ok(())
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                i = j + 1;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn extract_awaiting_prompt(output: &str, prompt: &str) -> String {
    let prompt = prompt.trim();
    let cleaned = strip_ansi(output);
    let lines: Vec<&str> = cleaned
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(20)
        .collect();

    let question_match = lines.iter().find(|line| line.ends_with('?'));
    if let Some(q) = question_match {
        return q.to_string();
    }

    let patterns = [
        "(y/n)",
        "(Y/n)",
        "(yes/no)",
        "(Yes/No)",
        "Do you want",
        "Would you like",
        "Shall I",
        "Should I",
        "Please confirm",
        "Continue?",
    ];
    for line in &lines {
        if line.starts_with('>') || line.starts_with('?') {
            return line.to_string();
        }
        for pattern in &patterns {
            if line.contains(pattern) {
                return line.to_string();
            }
        }
    }

    prompt.to_string()
}

fn mark_awaiting_input(
    store: &Arc<Store>,
    task_id: &TaskId,
    prompt: &str,
    awaiting_prompt: &str,
    awaiting_input: &mut bool,
) -> Result<()> {
    if *awaiting_input {
        return Ok(());
    }
    store.update_task_status(task_id.as_str(), TaskStatus::AwaitingInput)?;
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Reasoning,
        detail: prompt.to_string(),
        metadata: Some(json!({ "awaiting_input": true, "awaiting_prompt": awaiting_prompt })),
    })?;
    *awaiting_input = true;
    Ok(())
}

fn write_output_file(path: &str, buffer: &str) -> Result<()> {
    if let Some(response) = crate::agent::gemini::extract_response(buffer) {
        std::fs::write(path, response)?;
    } else {
        std::fs::write(path, buffer)?;
    }
    Ok(())
}

fn persist_transcript(task_id: &TaskId, buffer: &str) {
    let _ = std::fs::create_dir_all(crate::paths::task_dir(task_id.as_str()));
    let _ = std::fs::write(crate::paths::transcript_path(task_id.as_str()), buffer);
}

fn load_monitor_status(store: &Store, task_id: &str) -> Result<MonitorTaskStatus> {
    let status = store.get_task(task_id)?.map(|task| task.status);
    Ok(match status {
        Some(TaskStatus::Running) => MonitorTaskStatus::Running,
        Some(TaskStatus::AwaitingInput) => MonitorTaskStatus::AwaitingInput,
        Some(TaskStatus::Stalled) => MonitorTaskStatus::Stalled,
        _ => MonitorTaskStatus::Inactive,
    })
}

#[cfg(test)]
mod tests;
