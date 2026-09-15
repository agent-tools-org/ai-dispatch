// PTY idle prompt detection gated by buffered-agent log liveness.
// Extends MonitorState with prompt transitions and recovery on subsequent log growth.
// Deps: prompt, paths, task_lifecycle, and Store.

use super::{MonitorState, extract_awaiting_prompt, mark_awaiting_input};
use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId};
use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

impl MonitorState {
    pub(super) fn mark_prompt(
        &mut self,
        store: &Arc<Store>,
        task_id: &TaskId,
        prompt: &str,
    ) -> Result<()> {
        if self.awaiting_input {
            return Ok(());
        }
        let awaiting_prompt = extract_awaiting_prompt(&self.full_output, prompt);
        mark_awaiting_input(store, task_id, prompt, &awaiting_prompt, &mut self.awaiting_input)?;
        self.awaiting_log_sizes = log_sizes(task_id);
        Ok(())
    }

    pub(super) fn handle_timeout(&mut self, store: &Arc<Store>, task_id: &TaskId) -> Result<()> {
        if self.streaming {
            return Ok(());
        }
        if self.awaiting_input && log_sizes(task_id).iter()
            .zip(self.awaiting_log_sizes).any(|(size, previous)| *size > previous)
        {
            self.finish_input_delivery(store, task_id)?;
            store.insert_event(&TaskEvent {
                task_id: task_id.clone(),
                timestamp: Local::now(),
                event_kind: EventKind::Reasoning,
                detail: "Resumed running: agent log grew after awaiting input".to_string(),
                metadata: Some(json!({ "awaiting_input": false, "reason": "agent_log_growth" })),
            })?;
            return Ok(());
        }
        if Self::buffered_log_grew_within(task_id.as_str(), self.idle_detector.warn_after) {
            return Ok(());
        }
        if let Some(prompt) = self.prompt_detector.poll_idle(Instant::now()) {
            self.mark_prompt(store, task_id, &prompt)?;
        }
        Ok(())
    }
}

fn log_sizes(task_id: &TaskId) -> [u64; 3] {
    crate::paths::agent_byte_paths(task_id.as_str())
        .map(|path| std::fs::metadata(path).map(|meta| meta.len()).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;
    use crate::pty_watch::tests::pty_task;
    use crate::types::TaskStatus;
    use std::time::{Duration, SystemTime};

    const BUFFERED_PROGRESS: &str = "I'll start by exploring the codebase structure...";

    fn fixture() -> (tempfile::TempDir, paths::AidHomeGuard, Arc<Store>, TaskId, MonitorState) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = paths::AidHomeGuard::set(temp.path());
        paths::ensure_dirs().expect("dirs");
        let store = Arc::new(Store::open_memory().expect("store"));
        let task = pty_task("t-buffered-prompt", TaskStatus::Running);
        store.insert_task(&task).expect("task");
        std::fs::create_dir_all(paths::task_dir(task.id.as_str())).expect("task dir");
        (temp, home, store, task.id, MonitorState::new(false, None))
    }

    fn partial(state: &mut MonitorState, text: &str) {
        state.full_output = text.to_string();
        assert_eq!(state.prompt_detector.push_chunk(
            text, Instant::now() - Duration::from_secs(3),
        ), None);
    }

    fn status(store: &Store, id: &TaskId) -> TaskStatus {
        store.get_task(id.as_str()).expect("task").expect("exists").status
    }

    #[test]
    fn buffered_progress_defers_idle_prompt_until_logs_are_silent() {
        let (_temp, _home, store, id, mut state) = fixture();
        partial(&mut state, BUFFERED_PROGRESS);
        let path = paths::agent_log_path(id.as_str());
        std::fs::write(&path, "working").expect("log");
        state.handle_timeout(&store, &id).expect("timeout");
        assert_eq!(status(&store, &id), TaskStatus::Running);
        assert!(store.get_events(id.as_str()).expect("events").is_empty());

        std::fs::File::open(path).expect("log").set_modified(
            SystemTime::now() - state.idle_detector.warn_after - Duration::from_secs(10),
        ).expect("age log");
        state.handle_timeout(&store, &id).expect("silent timeout");
        assert_eq!(status(&store, &id), TaskStatus::AwaitingInput);
        let events = store.get_events(id.as_str()).expect("events");
        assert_eq!(events[0].metadata.as_ref().expect("metadata")["awaiting_prompt"], BUFFERED_PROGRESS);
    }

    #[test]
    fn measured_model_latency_gaps_do_not_trigger_await() {
        let (_temp, _home, store, id, mut state) = fixture();
        partial(&mut state, BUFFERED_PROGRESS);
        let path = paths::agent_log_path(id.as_str());
        std::fs::write(&path, "working").expect("log");
        for gap_secs in [3, 31, 145] {
            std::fs::File::open(&path).expect("log").set_modified(
                SystemTime::now() - Duration::from_secs(gap_secs),
            ).expect("age log");
            state.handle_timeout(&store, &id).expect("model latency");
            assert_eq!(status(&store, &id), TaskStatus::Running, "gap: {gap_secs}s");
        }
        assert!(store.get_events(id.as_str()).expect("events").is_empty());
    }

    #[test]
    fn idle_prompt_uses_warning_window_from_task_policy() {
        let (_temp, _home, store, id, _) = fixture();
        let mut policy = crate::timeout_policy::TimeoutPolicy::default();
        policy.nudge_ladder.warn = Duration::from_secs(10);
        let mut state = MonitorState::with_policy(false, None, policy);
        partial(&mut state, BUFFERED_PROGRESS);
        let path = paths::agent_log_path(id.as_str());
        std::fs::write(&path, "working").expect("log");
        for (age, expected) in [(8, TaskStatus::Running), (15, TaskStatus::AwaitingInput)] {
            std::fs::File::open(&path).expect("log").set_modified(
                SystemTime::now() - Duration::from_secs(age),
            ).expect("age log");
            state.handle_timeout(&store, &id).expect("timeout");
            assert_eq!(status(&store, &id), expected);
        }
    }

    #[test]
    fn subsequent_log_growth_clears_await_and_resets_detector_once() {
        let (_temp, _home, store, id, mut state) = fixture();
        partial(&mut state, BUFFERED_PROGRESS);
        state.handle_timeout(&store, &id).expect("await");
        assert!(state.awaiting_input);
        std::fs::write(paths::agent_log_path(id.as_str()), "working").expect("log");
        state.handle_timeout(&store, &id).expect("resume");
        assert_eq!(status(&store, &id), TaskStatus::Running);
        assert!(!state.awaiting_input);
        assert_eq!(state.prompt_detector.poll_idle(Instant::now() + Duration::from_secs(10)), None);
        state.handle_timeout(&store, &id).expect("repeat");
        let events = store.get_events(id.as_str()).expect("events");
        assert_eq!(events.len(), 2);
        assert!(events[1].detail.contains("agent log grew"));
        assert_eq!(events[1].metadata.as_ref().expect("metadata")["awaiting_input"], false);
    }

    #[test]
    fn delayed_poll_still_recovers_after_log_growth() {
        let (_temp, _home, store, id, mut state) = fixture();
        partial(&mut state, BUFFERED_PROGRESS);
        state.handle_timeout(&store, &id).expect("await");
        let path = paths::agent_log_path(id.as_str());
        std::fs::write(&path, "working").expect("log");
        std::fs::File::open(path).expect("log").set_modified(
            SystemTime::now() - state.idle_detector.warn_after - Duration::from_secs(10),
        ).expect("age log");
        state.handle_timeout(&store, &id).expect("delayed resume");
        assert_eq!(status(&store, &id), TaskStatus::Running);
        assert!(!state.awaiting_input);
    }

    #[test]
    fn immediate_prompt_with_recent_unchanged_log_remains_awaiting() {
        let (_temp, _home, store, id, mut state) = fixture();
        std::fs::write(paths::log_path(id.as_str()), "Proceed? (y/n) ").expect("pty log");
        state.mark_prompt(&store, &id, "Proceed? (y/n)").expect("prompt");
        state.handle_timeout(&store, &id).expect("timeout");
        assert_eq!(status(&store, &id), TaskStatus::AwaitingInput);
        assert_eq!(store.get_events(id.as_str()).expect("events").len(), 1);
    }

    #[test]
    fn idle_prompt_without_log_still_awaits() {
        let (_temp, _home, store, id, mut state) = fixture();
        partial(&mut state, "Select an option");
        state.handle_timeout(&store, &id).expect("timeout");
        assert_eq!(status(&store, &id), TaskStatus::AwaitingInput);
    }

    #[test]
    fn streaming_timeout_does_not_detect_partial_prompts() {
        let (_temp, _home, store, id, mut state) = fixture();
        state.streaming = true;
        partial(&mut state, BUFFERED_PROGRESS);
        state.handle_timeout(&store, &id).expect("timeout");
        assert_eq!(status(&store, &id), TaskStatus::Running);
    }
}
