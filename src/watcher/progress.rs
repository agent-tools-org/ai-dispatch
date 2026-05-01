// Watcher progress helpers for synthetic milestones and loop detection.
// Exports tracker types shared by watcher flows and PTY monitoring.

use std::collections::{HashMap, VecDeque};

use chrono::Local;

use crate::types::{EventKind, TaskEvent, TaskId};

const SYNTHETIC_PROGRESS_WINDOW: usize = 10;

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

pub(super) struct LoopDetector {
    recent_events: VecDeque<String>,
    file_write_counts: HashMap<String, usize>,
    last_file_write_key: Option<String>,
}

impl LoopDetector {
    pub(super) fn new() -> Self {
        Self {
            recent_events: VecDeque::new(),
            file_write_counts: HashMap::new(),
            last_file_write_key: None,
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

        if kind == EventKind::FileWrite {
            self.push_file_write(key);
            return;
        }

        self.reset_file_write_counts();
        self.recent_events.push_back(key.to_string());
        if self.recent_events.len() > 20 {
            self.recent_events.pop_front();
        }
    }

    pub(super) fn is_looping(&self) -> bool {
        if self.file_write_counts.values().any(|count| *count >= 15) {
            return true;
        }
        if self.recent_events.len() < 10 {
            return false;
        }
        let mut counts = HashMap::new();
        for detail in self.recent_events.iter().rev().take(10) {
            let counter = counts.entry(detail.as_str()).or_insert(0);
            *counter += 1;
            if *counter >= 8 {
                return true;
            }
        }
        false
    }

    fn push_file_write(&mut self, key: &str) {
        if self.last_file_write_key.as_deref() != Some(key) {
            self.file_write_counts.clear();
            self.file_write_counts.insert(key.to_string(), 1);
            self.last_file_write_key = Some(key.to_string());
            return;
        }

        let counter = self.file_write_counts.entry(key.to_string()).or_insert(0);
        *counter += 1;
    }

    fn reset_file_write_counts(&mut self) {
        self.file_write_counts.clear();
        self.last_file_write_key = None;
    }
}
