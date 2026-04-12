// WAIT timeout tracking for batch dispatch.
// Exports ReadyWaitTracker to fail ready-but-unscheduled tasks after max_wait_mins.
// Deps: batch config types, Store, chrono.

use anyhow::Result;
use chrono::{DateTime, Local};
use std::sync::Arc;

use crate::batch;
use crate::store::Store;
use crate::types::TaskStatus;

pub(super) struct ReadyWaitTracker {
    ready_since: Vec<Option<DateTime<Local>>>,
    max_wait_mins: Vec<Option<u64>>,
}

impl ReadyWaitTracker {
    pub(super) fn new(tasks: &[batch::BatchTask]) -> Self {
        Self {
            ready_since: vec![None; tasks.len()],
            max_wait_mins: tasks
                .iter()
                .map(effective_max_wait_mins)
                .collect(),
        }
    }

    pub(super) fn observe_ready(&mut self, ready: &[usize], now: DateTime<Local>) {
        for &task_idx in ready {
            if self.ready_since[task_idx].is_none() {
                self.ready_since[task_idx] = Some(now);
            }
        }
    }

    pub(super) fn clear(&mut self, task_idx: usize) {
        self.ready_since[task_idx] = None;
    }

    pub(super) fn fail_expired(
        &mut self,
        store: &Arc<Store>,
        waiting_ids: &[String],
        started: &[bool],
        now: DateTime<Local>,
    ) -> Result<Vec<String>> {
        let mut expired = Vec::new();
        for task_idx in 0..self.ready_since.len() {
            if started[task_idx] || !self.is_expired(task_idx, now) {
                continue;
            }
            let task_id = &waiting_ids[task_idx];
            let Some(task) = store.get_task(task_id)? else {
                self.clear(task_idx);
                continue;
            };
            if task.status != TaskStatus::Waiting {
                self.clear(task_idx);
                continue;
            }
            let detail = self.timeout_detail(task_idx, now);
            if store.fail_waiting_with_reason(task_id, &detail)? {
                expired.push(task_id.clone());
            }
            self.clear(task_idx);
        }
        Ok(expired)
    }

    fn is_expired(&self, task_idx: usize, now: DateTime<Local>) -> bool {
        let Some(limit_mins) = self.max_wait_mins[task_idx] else {
            return false;
        };
        let Some(ready_since) = self.ready_since[task_idx].as_ref() else {
            return false;
        };
        (now - ready_since.clone()).num_seconds() >= (limit_mins * 60) as i64
    }

    fn timeout_detail(&self, task_idx: usize, now: DateTime<Local>) -> String {
        let ready_since = self.ready_since[task_idx].as_ref().cloned().unwrap_or(now);
        let elapsed_secs = (now - ready_since).num_seconds().max(0);
        let limit_mins = self.max_wait_mins[task_idx].unwrap_or_default();
        format!(
            "wait timeout: no agent slot available after {}s (limit {}m)",
            elapsed_secs, limit_mins
        )
    }
}

fn effective_max_wait_mins(task: &batch::BatchTask) -> Option<u64> {
    task.max_wait_mins.filter(|mins| *mins > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_wait_uses_only_explicit_max_wait_mins() {
        let mut task = stub_task();
        assert_eq!(effective_max_wait_mins(&task), None);
        task.max_duration_mins = Some(20);
        assert_eq!(effective_max_wait_mins(&task), None);
        task.max_wait_mins = Some(5);
        assert_eq!(effective_max_wait_mins(&task), Some(5));
        task.max_wait_mins = Some(0);
        assert_eq!(effective_max_wait_mins(&task), None);
    }

    fn stub_task() -> batch::BatchTask {
        batch::BatchTask {
            id: None,
            name: None,
            agent: "codex".to_string(),
            team: None,
            prompt: "test".to_string(),
            prompt_file: None,
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            setup: None,
            judge: None,
            peer_review: None,
            max_duration_mins: None,
            max_wait_mins: None,
            retry: None,
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            idle_timeout: None,
            best_of: None,
            metric: None,
            context: None,
            checklist: None,
            skills: None,
            on_done: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            sandbox: false,
            no_skill: false,
            budget: false,
            audit: None,
            env: None,
            env_forward: None,
            worktree_link_deps: None,
            on_success: None,
            on_fail: None,
            conditional: false,
        }
    }
}
