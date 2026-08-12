// PTY monitoring helpers for interactive background tasks.
// Owns chunk parsing, prompt detection, input forwarding, and completion finalization.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::collections::HashSet;
use std::io::Write;
use std::sync::{Arc, mpsc::{self, RecvTimeoutError}};
use std::time::{Duration, Instant};

use crate::agent::Agent;
use crate::delivery_guard::{DeliveryEvidence, DeliveryOutcome};
use crate::input_signal;
use crate::process_monitor;
use crate::prompt::PromptDetector;
use crate::pty_bridge::PtyBridge;
use crate::pty_watch_idle::{
    is_agent_output, load_monitor_status, register_inbound_echo, take_inbound_echo, InboundEcho,
    IdleAction, IdleDetector,
};
use crate::store::Store;
use crate::types::{AgentKind, CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};
use crate::watcher::{self, SyntheticMilestoneTracker};

mod utf8;

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
    last_progress_time: Instant,
    last_raw_chunk_time: Instant,
    /// Set on the first real PTY byte chunk. Buffered agents never raise
    /// `event_count`, so this is how first-token knows "silence since spawn"
    /// apart from "silence after progress".
    received_raw_bytes: bool,
    /// Wall-clock time this state was created. Used by the first-token hang
    /// check to exclude log files left by earlier attempts on the same task id.
    start_system_time: std::time::SystemTime,
    idle_nudged: bool,
    idle_warned: bool,
    pending_inbound_acks: usize,
    failed_inbound_message_ids: HashSet<i64>,
    /// Payloads aid wrote to the PTY; matching stream lines are echoes, not agent progress.
    inbound_echo_suppress: Vec<InboundEcho>,
    idle_detector: IdleDetector,
    streaming: bool,
    workgroup_id: Option<String>,
    session_saved: bool,
    delivery_evidence: DeliveryEvidence,
    saw_completion_event: bool,
}

impl MonitorState {
    pub(crate) fn new(streaming: bool, workgroup_id: Option<String>) -> Self {
        Self::with_policy(
            streaming,
            workgroup_id,
            crate::timeout_policy::TimeoutPolicy::default(),
        )
    }

    pub(crate) fn with_policy(
        streaming: bool,
        workgroup_id: Option<String>,
        timeout_policy: crate::timeout_policy::TimeoutPolicy,
    ) -> Self {
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
            last_progress_time: Instant::now(),
            last_raw_chunk_time: Instant::now(),
            received_raw_bytes: false,
            start_system_time: std::time::SystemTime::now(),
            idle_nudged: false,
            idle_warned: false,
            pending_inbound_acks: 0,
            failed_inbound_message_ids: HashSet::new(),
            inbound_echo_suppress: Vec::new(),
            idle_detector: IdleDetector::from_policy(timeout_policy),
            streaming,
            workgroup_id,
            session_saved: false,
            delivery_evidence: DeliveryEvidence::default(),
            saw_completion_event: false,
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
        let chunk = watcher::strip_terminal_escapes(&chunk);
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
            let is_echo = take_inbound_echo(&mut self.inbound_echo_suppress, &line);
            if !is_echo && agent.kind() == AgentKind::Codex {
                self.delivery_evidence.observe_codex_jsonl(&line);
            }
            self.observe_output_line(task_id, store, &line)?;
            if !is_echo && is_agent_output(&line) {
                self.mark_progress();
            }
            if self.streaming && !is_echo {
                if let Some(event_detail) = watcher::handle_streaming_line_with_session(
                    watcher::StreamLineContext {
                        agent,
                        task_id,
                        store,
                        workgroup_id: self.workgroup_id.as_deref(),
                        synthetic_tracker: &mut self.synthetic_tracker,
                    },
                    &mut self.info,
                    &mut self.event_count,
                    &line,
                    &mut self.session_saved,
                )? {
                    if event_detail.kind == EventKind::Completion {
                        self.saw_completion_event = true;
                    }
                    if event_detail.kind.is_liveness() {
                        self.last_event_detail = Some(event_detail.detail);
                    }
                }
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
        let is_echo = take_inbound_echo(&mut self.inbound_echo_suppress, &trailing);
        if !is_echo && agent.kind() == AgentKind::Codex {
            self.delivery_evidence.observe_codex_jsonl(&trailing);
        }
        self.observe_output_line(task_id, store, &trailing)?;
        if !is_echo && is_agent_output(&trailing) {
            self.mark_progress();
        }
        if self.streaming && !is_echo {
            if let Some(event_detail) = watcher::handle_streaming_line_with_session(
                watcher::StreamLineContext {
                    agent,
                    task_id,
                    store,
                    workgroup_id: self.workgroup_id.as_deref(),
                    synthetic_tracker: &mut self.synthetic_tracker,
                },
                &mut self.info,
                &mut self.event_count,
                &trailing,
                &mut self.session_saved,
            )? {
                if event_detail.kind == EventKind::Completion {
                    self.saw_completion_event = true;
                }
                if event_detail.kind.is_liveness() {
                    self.mark_progress();
                    self.last_event_detail = Some(event_detail.detail);
                }
            }
        }
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
        if !Self::write_input_or_record_failure(
            bridge, store, task_id, "Response", &input, None,
        )? {
            return Ok(());
        }
        register_inbound_echo(&mut self.inbound_echo_suppress, input);
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
        if !Self::write_input_or_record_failure(
            bridge, store, task_id, "Steer", &message, None,
        )? {
            return Ok(());
        }
        register_inbound_echo(&mut self.inbound_echo_suppress, message.clone());
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
            if self.failed_inbound_message_ids.contains(&message.id) {
                continue;
            }
            if !Self::write_input_or_record_failure(
                bridge,
                store,
                task_id,
                "Reply",
                &message.content,
                Some(message.id),
            )? {
                self.failed_inbound_message_ids.insert(message.id);
                break;
            }
            register_inbound_echo(&mut self.inbound_echo_suppress, message.content.clone());
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

    fn write_input_or_record_failure(
        bridge: &mut PtyBridge,
        store: &Arc<Store>,
        task_id: &TaskId,
        source: &str,
        message: &str,
        message_id: Option<i64>,
    ) -> Result<bool> {
        let delivery = if bridge.is_alive() {
            bridge.write_input(message)
        } else {
            Err(anyhow::anyhow!("PTY child has already exited"))
        };
        let Err(error) = delivery else {
            return Ok(true);
        };
        store.insert_event(&TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: format!(
                "{source} not delivered: {} — {error:#}",
                message.chars().take(200).collect::<String>()
            ),
            metadata: Some(json!({
                "input_delivery": "failed",
                "delivered": false,
                "source": source,
                "message_id": message_id,
                "error": error.to_string(),
            })),
        })?;
        Ok(false)
    }

    fn maybe_handle_idle(
        &mut self,
        store: &Arc<Store>,
        task_id: &TaskId,
        accepts_nudge: bool,
    ) -> Result<()> {
        // Same buffered-liveness question as idle_hang_elapsed — read only.
        // Do not mark_progress(): that clock belongs to the hang reaper.
        if !self.streaming
            && Self::buffered_log_grew_within(task_id.as_str(), self.idle_detector.warn_after)
        {
            return Ok(());
        }
        match self.idle_detector.tick(
            self.last_progress_time,
            load_monitor_status(store.as_ref(), task_id.as_str())?,
            self.idle_nudged,
            accepts_nudge,
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
        crate::task_lifecycle::mark_running(store.as_ref(), task_id)?;
        self.awaiting_input = false;
        self.prompt_detector.reset_after_input();
        Ok(())
    }

    fn progress_count(&self) -> u32 {
        self.event_count
    }

    fn first_token_hang_elapsed(
        &self,
        agent_streaming: bool,
        first_token_timeout: Duration,
        task_id: &str,
    ) -> bool {
        // Streaming: still at zero/one parsed event. Buffered: never saw a byte
        // on the PTY AND no agent log file has grown since spawn.
        // Silence after progress keeps the long idle budget either way.
        //
        // Buffered agents (grok, agy) write nothing to the PTY until they exit,
        // so `received_raw_bytes` stays false for the whole run. They do write to
        // their own log file (grok via --debug-file, agy via --log-file). Checking
        // that file here keeps them alive past the first-token budget, matching
        // exactly what the orphan reaper does in background_orphan.rs.
        let awaiting_first_output = if agent_streaming {
            self.progress_count() <= 1
        } else {
            !self.received_raw_bytes
                && !crate::paths::agent_has_produced_bytes(task_id, self.start_system_time)
        };
        awaiting_first_output && self.last_raw_chunk_time.elapsed() > first_token_timeout
    }

    /// Idle hang for the live PTY watcher. Streaming keeps the progress-event
    /// clock alone. Buffered agents write nothing to the PTY mid-run, so a quiet
    /// `last_progress_time` is not enough — also require that agent-owned logs
    /// have not grown within the idle window (same `agent_has_produced_bytes`
    /// question the first-token detector and orphan reaper ask).
    fn idle_hang_elapsed(
        &self,
        agent_streaming: bool,
        idle: Duration,
        task_id: &str,
    ) -> bool {
        self.idle_hang_elapsed_at(agent_streaming, idle, task_id, Instant::now())
    }

    /// Same decision as [`Self::idle_hang_elapsed`], with an injectable `now`
    /// so boundary tests can pin the clock (production always passes
    /// `Instant::now()` via the wrapper).
    fn idle_hang_elapsed_at(
        &self,
        agent_streaming: bool,
        idle: Duration,
        task_id: &str,
        now: Instant,
    ) -> bool {
        if now.saturating_duration_since(self.last_progress_time) <= idle {
            return false;
        }
        if agent_streaming {
            return true;
        }
        !Self::buffered_log_grew_within(task_id, idle)
    }

    /// True when any agent-owned log grew inside `window`. Shared by the hang
    /// reaper and the warn/nudge/escalate ladder — do not invent another ask.
    fn buffered_log_grew_within(task_id: &str, window: Duration) -> bool {
        let window_start = std::time::SystemTime::now()
            .checked_sub(window)
            .unwrap_or(std::time::UNIX_EPOCH);
        crate::paths::agent_has_produced_bytes(task_id, window_start)
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

    fn mark_progress(&mut self) {
        self.last_progress_time = Instant::now();
        self.idle_warned = false;
        self.idle_nudged = false;
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
    first_token_timeout: Option<Duration>,
    idle_timeout: Option<Duration>,
) -> Result<()> {
    let mut reader_done = false;
    let mut child_exited_at: Option<Instant> = None;
    let mut decoder = utf8::Utf8Chunks::default();
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
                state.received_raw_bytes = true;
                state.last_raw_chunk_time = Instant::now();
                state.handle_chunk(agent, task_id, store, log_file, decoder.push(bytes))?;
            }
            Err(RecvTimeoutError::Timeout) => {
                state.handle_timeout(store, task_id)?;
                if let Some(first_token) = first_token_timeout
                    && state.first_token_hang_elapsed(agent.streaming(), first_token, task_id.as_str())
                {
                    state.info.status = TaskStatus::Failed;
                    process_monitor::insert_hung_detected_events(
                        store.as_ref(),
                        task_id,
                        first_token.as_secs(),
                        state.progress_count(),
                        state.last_progress_detail().as_deref(),
                        true,
                    )?;
                    break;
                }
                if let Some(idle) = idle_timeout
                    && state.idle_hang_elapsed(agent.streaming(), idle, task_id.as_str())
                {
                    state.info.status = TaskStatus::Failed;
                    process_monitor::insert_hung_detected_events(
                        store.as_ref(),
                        task_id,
                        idle.as_secs(),
                        state.progress_count(),
                        state.last_progress_detail().as_deref(),
                        false,
                    )?;
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => { state.handle_chunk(agent, task_id, store, log_file, decoder.flush())?; reader_done = true; }
        }
        let accepts_input = agent.accepts_interactive_input();
        if accepts_input {
            state.maybe_forward_input(bridge, store, task_id)?;
            state.maybe_forward_steer(bridge, store, task_id)?;
            state.maybe_consume_reply(bridge, store, task_id)?;
        }
        let accepts_nudge = accepts_input && agent.accepts_idle_nudge();
        state.maybe_handle_idle(store, task_id, accepts_nudge)?;
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

fn finalize_streaming(
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
    if crate::agent::stream_completion::record_quota_exhaustion_with_delivery(
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
    store.insert_event(&crate::agent::gemini::make_completion_event(
        task_id,
        &state.info,
    ))?;
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
    append_terminal_sentinel_line(&crate::paths::transcript_path(task_id.as_str()), &sentinel);
}

pub(crate) fn append_stopped_terminal_sentinel(
    task_id: &TaskId,
    log_path: &std::path::Path,
    reason: &str,
) {
    let sentinel = format!("\n=== AID TASK {} STOPPED ({}) ===\n", task_id, reason);
    append_terminal_sentinel_line(log_path, &sentinel);
    append_terminal_sentinel_line(&crate::paths::transcript_path(task_id.as_str()), &sentinel);
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

fn extract_awaiting_prompt(output: &str, prompt: &str) -> String {
    let prompt = prompt.trim();
    let cleaned = crate::watcher::strip_terminal_escapes(output);
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
    crate::task_lifecycle::mark_awaiting_input(store.as_ref(), task_id)?;
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

fn write_output_file(agent: AgentKind, path: &str, buffer: &str) -> Result<()> {
    if let Some(response) = crate::agent::extract_response(agent, buffer) {
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

#[cfg(test)]
mod tests;
#[cfg(test)]
mod log_tests;
#[cfg(test)]
mod first_token_tests;
#[cfg(test)]
mod idle_hang_tests;
#[cfg(test)]
#[path = "pty_watch_activity_tests.rs"]
mod activity_tests;
#[cfg(test)]
#[path = "pty_watch_write_tests.rs"]
mod write_tests;
