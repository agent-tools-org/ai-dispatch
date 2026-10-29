// PTY monitoring helpers for interactive background tasks.
// Owns chunk parsing, prompt detection, input forwarding, and completion finalization.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::collections::HashSet;
use std::io::Write;
use std::sync::{
    Arc,
    mpsc::{self, RecvTimeoutError},
};
use std::time::{Duration, Instant};

use crate::agent::Agent;
use crate::prompt::PromptDetector;
use crate::runtime::input_signal;
use crate::runtime::process;
use crate::runtime::pty::bridge::PtyBridge;
use crate::runtime::pty::watch_idle::{
    IdleAction, IdleDetector, InboundEcho, is_agent_output, load_monitor_status,
    register_inbound_echo, take_inbound_echo,
};
use crate::runtime::watcher::{self, SyntheticMilestoneTracker};
use crate::store::Store;
use crate::task::delivery_guard::{DeliveryEvidence, DeliveryOutcome};
use crate::types::{AgentKind, CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};

pub(crate) mod piped;
mod prompt_liveness;
mod utf8;

mod finalize;
mod input;
mod monitor;
mod output;
#[cfg(test)]
use finalize::finalize_streaming;
pub(crate) use finalize::{append_failed_terminal_sentinel, finalize_output};
pub(crate) use monitor::monitor_bridge;
use output::{extract_awaiting_prompt, mark_awaiting_input, persist_transcript, write_output_file};

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) struct MonitorState {
    pub(crate) info: CompletionInfo,
    pub(crate) absolute_deadline: Instant,
    full_output: String,
    line_buffer: String,
    event_count: u32,
    last_event_detail: Option<String>,
    synthetic_tracker: SyntheticMilestoneTracker,
    prompt_detector: PromptDetector,
    awaiting_input: bool,
    awaiting_log_sizes: [u64; 3],
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
    #[cfg(test)]
    pub(crate) fn new(streaming: bool, workgroup_id: Option<String>) -> Self {
        Self::with_policy(
            streaming,
            workgroup_id,
            crate::config::timeout_policy::TimeoutPolicy::default(),
        )
    }

    pub(crate) fn with_policy(
        streaming: bool,
        workgroup_id: Option<String>,
        timeout_policy: crate::config::timeout_policy::TimeoutPolicy,
    ) -> Self {
        Self {
            absolute_deadline: Instant::now() + timeout_policy.max_duration.min(timeout_policy.hard_cap),
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
            awaiting_log_sizes: [0; 3],
            last_progress_time: Instant::now(),
            last_raw_chunk_time: Instant::now(),
            received_raw_bytes: false,
            start_system_time: std::time::SystemTime::now(),
            idle_nudged: false,
            idle_warned: false,
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
                store.insert_event(&TaskEvent::new(
                    task_id.clone(),
                    EventKind::Reasoning,
                    "idle warn".to_string(),
                ).with_metadata(Some(json!({ "idle_warn": true }))))?;
            }
            IdleAction::WarnEvent => {}
            IdleAction::SendNudge(message) => {
                self.queue_idle_nudge(store, task_id, message)?;
            }
            IdleAction::Escalate
                if crate::task::unstick::mark_task_stalled(store.as_ref(), task_id.as_str())? =>
            {
                store.insert_event(&TaskEvent::new(
                    task_id.clone(),
                    EventKind::Milestone,
                    "Auto-escalated: task stalled".to_string(),
                ).with_metadata(Some(json!({ "auto_escalated": true }))))?;
            }
            IdleAction::Escalate => {}
        }
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
                && !crate::support::paths::agent_has_produced_bytes(task_id, self.start_system_time)
        };
        awaiting_first_output && self.last_raw_chunk_time.elapsed() > first_token_timeout
    }

    /// Idle hang for the live PTY watcher. Streaming keeps the progress-event
    /// clock alone. Buffered agents write nothing to the PTY mid-run, so a quiet
    /// `last_progress_time` is not enough — also require that agent-owned logs
    /// have not grown within the idle window (same `agent_has_produced_bytes`
    /// question the first-token detector and orphan reaper ask).
    fn idle_hang_elapsed(&self, agent_streaming: bool, idle: Duration, task_id: &str) -> bool {
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
        crate::support::paths::agent_has_produced_bytes(task_id, window_start)
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

#[cfg(test)]
#[path = "watch_activity_tests.rs"]
mod activity_tests;
#[cfg(test)]
#[path = "watch_child_tests.rs"]
mod child_tests;
#[cfg(test)]
mod first_token_tests;
#[cfg(test)]
mod idle_hang_tests;
#[cfg(test)]
mod log_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "watch_write_tests.rs"]
mod write_tests;

#[cfg(test)]
mod input_status_tests;
