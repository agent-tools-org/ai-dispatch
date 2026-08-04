// Watcher progress helpers for synthetic milestones and loop detection.
// Exports tracker types shared by watcher flows and PTY monitoring.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use chrono::Local;

use crate::types::{EventKind, TaskEvent, TaskId};

const SYNTHETIC_PROGRESS_WINDOW: usize = 10;
const LOOP_MIN_DURATION: Duration = Duration::from_secs(120);
const RECENT_EVENT_LIMIT: usize = 20;
const LOOP_SAMPLE_SIZE: usize = 10;
const LOOP_REPEAT_THRESHOLD: usize = 8;
const FILE_WRITE_REPEAT_THRESHOLD: usize = 15;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SyntheticToolKind {
    Read,
    Edit,
    Execute,
    Other,
}

pub(crate) struct SyntheticMilestoneTracker {
    early_event_count: usize,
    synthetic_disabled: bool,
    consecutive_reads: usize,
    max_read_milestone: usize,
    edit_count: usize,
    max_edit_milestone: usize,
    saw_edit_after_read: bool,
}

impl SyntheticMilestoneTracker {
    pub(crate) fn new() -> Self {
        Self {
            early_event_count: 0,
            synthetic_disabled: false,
            consecutive_reads: 0,
            max_read_milestone: 0,
            edit_count: 0,
            max_edit_milestone: 0,
            saw_edit_after_read: false,
        }
    }

    pub(crate) fn observe(&mut self, event: &TaskEvent) {
        if self.early_event_count < SYNTHETIC_PROGRESS_WINDOW {
            self.early_event_count += 1;
            if matches!(event.event_kind, EventKind::Reasoning | EventKind::Milestone) {
                self.synthetic_disabled = true;
            }
        }
    }

    pub(crate) fn synthetic_event(&mut self, task_id: &TaskId, event: &TaskEvent) -> Option<TaskEvent> {
        if event.event_kind != EventKind::ToolCall || self.synthetic_disabled {
            return None;
        }

        let detail = match Self::tool_kind(&event.detail) {
            SyntheticToolKind::Read => self.read_milestone(),
            SyntheticToolKind::Edit => self.edit_milestone(),
            SyntheticToolKind::Execute => {
                self.consecutive_reads = 0;
                Some("[verifying] running command".to_string())
            }
            SyntheticToolKind::Other => {
                self.consecutive_reads = 0;
                None
            }
        }?;

        Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail,
            metadata: Some(serde_json::json!({ "synthetic": true })),
        })
    }

    fn tool_kind(detail: &str) -> SyntheticToolKind {
        let name = detail.split_once('(').map(|(head, _)| head).unwrap_or(detail).trim();
        if name.eq_ignore_ascii_case("Read") || name.eq_ignore_ascii_case("Glob") {
            SyntheticToolKind::Read
        } else if name.eq_ignore_ascii_case("Edit")
            || name.eq_ignore_ascii_case("Write")
            || name.eq_ignore_ascii_case("MultiEdit")
        {
            SyntheticToolKind::Edit
        } else if name.eq_ignore_ascii_case("Execute") || name.eq_ignore_ascii_case("Bash") {
            SyntheticToolKind::Execute
        } else {
            SyntheticToolKind::Other
        }
    }

    fn read_milestone(&mut self) -> Option<String> {
        self.consecutive_reads += 1;
        if self.consecutive_reads >= 3 && self.consecutive_reads > self.max_read_milestone {
            self.max_read_milestone = self.consecutive_reads;
            Some(format!("[exploring] read {} files", self.consecutive_reads))
        } else {
            None
        }
    }

    fn edit_milestone(&mut self) -> Option<String> {
        let first_edit = self.consecutive_reads > 0 && !self.saw_edit_after_read;
        self.consecutive_reads = 0;
        self.edit_count += 1;
        if first_edit {
            self.saw_edit_after_read = true;
            Some("[implementing] first edit".to_string())
        } else if self.edit_count >= 3 && self.edit_count > self.max_edit_milestone {
            self.max_edit_milestone = self.edit_count;
            Some(format!("[implementing] modified {} files", self.edit_count))
        } else {
            None
        }
    }
}

struct LoopObservation {
    key: String,
    observed_at: Instant,
}

pub(super) struct LoopDetector<C = fn() -> Instant> {
    clock: C,
    recent_events: VecDeque<LoopObservation>,
    file_write_count: usize,
    last_file_write_key: Option<String>,
    file_write_started_at: Option<Instant>,
    last_file_write_at: Option<Instant>,
}

impl LoopDetector<fn() -> Instant> {
    pub(super) fn new() -> Self {
        Self::with_clock(Instant::now)
    }
}

impl<C: Fn() -> Instant> LoopDetector<C> {
    pub(super) fn with_clock(clock: C) -> Self {
        Self {
            clock,
            recent_events: VecDeque::new(),
            file_write_count: 0,
            last_file_write_key: None,
            file_write_started_at: None,
            last_file_write_at: None,
        }
    }

    pub(super) fn push(&mut self, detail: &str, kind: EventKind, raw_key: Option<&str>) {
        let key = raw_key.unwrap_or(detail);
        if key.trim().is_empty() {
            if kind != EventKind::FileWrite {
                self.reset_file_write_counts();
            }
            return;
        }

        let observed_at = (self.clock)();
        if kind == EventKind::FileWrite {
            self.push_file_write(key, observed_at);
            return;
        }

        self.reset_file_write_counts();
        if !Self::is_loop_evidence(kind) {
            return;
        }
        self.recent_events.push_back(LoopObservation { key: key.to_string(), observed_at });
        if self.recent_events.len() > RECENT_EVENT_LIMIT {
            self.recent_events.pop_front();
        }
    }

    pub(super) fn is_looping(&self) -> bool {
        if self.file_write_count >= FILE_WRITE_REPEAT_THRESHOLD
            && self.file_write_persisted()
        {
            return true;
        }
        if self.recent_events.len() < LOOP_SAMPLE_SIZE {
            return false;
        }
        self.repeated_run_persisted()
    }

    fn push_file_write(&mut self, key: &str, observed_at: Instant) {
        if self.last_file_write_key.as_deref() != Some(key) {
            self.file_write_count = 1;
            self.last_file_write_key = Some(key.to_string());
            self.file_write_started_at = Some(observed_at);
            self.last_file_write_at = Some(observed_at);
            return;
        }

        self.file_write_count += 1;
        self.last_file_write_at = Some(observed_at);
    }

    fn reset_file_write_counts(&mut self) {
        self.file_write_count = 0;
        self.last_file_write_key = None;
        self.file_write_started_at = None;
        self.last_file_write_at = None;
    }

    fn is_loop_evidence(kind: EventKind) -> bool {
        matches!(
            kind,
            EventKind::ToolCall
                | EventKind::FileRead
                | EventKind::Build
                | EventKind::Test
                | EventKind::Commit
        )
    }

    fn file_write_persisted(&self) -> bool {
        match (self.file_write_started_at, self.last_file_write_at) {
            (Some(started_at), Some(last_at)) => last_at.duration_since(started_at) >= LOOP_MIN_DURATION,
            _ => false,
        }
    }

    fn repeated_run_persisted(&self) -> bool {
        let mut repeats: HashMap<&str, (usize, Instant)> = HashMap::new();
        for observation in self.recent_events.iter().rev().take(LOOP_SAMPLE_SIZE) {
            let entry = repeats.entry(observation.key.as_str()).or_insert((0, observation.observed_at));
            entry.0 += 1;
            if entry.0 >= LOOP_REPEAT_THRESHOLD
                && entry.1.duration_since(observation.observed_at) >= LOOP_MIN_DURATION
            {
                return true;
            }
        }
        false
    }
}
