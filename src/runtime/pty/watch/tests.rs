// Tests for PTY awaiting-input prompt capture.
// Covers metadata written for board-facing AWAIT reasons.
// Depends on pty_watch helpers and the in-memory Store.

use super::{MonitorState, extract_awaiting_prompt, finalize_streaming, mark_awaiting_input};
use crate::store::Store;
use crate::support::paths;
use crate::types::{AgentKind, Task, TaskId, TaskStatus};

use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) fn pty_task(task_id: &str, status: TaskStatus) -> Task {
    crate::types::Task::test_fixture(TaskId(task_id.to_string()), status)
}

#[path = "prompt_tests.rs"]
mod prompt_tests;

#[path = "progress_tests.rs"]
mod progress_tests;

#[path = "transcript_tests.rs"]
mod transcript_tests;
