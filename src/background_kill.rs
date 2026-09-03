// Shared reaper process termination helpers.
// Exports task-process target calculation and termination.
// Deps: background process signals and run specs.

#[cfg(not(test))]
use super::background_process::{kill_process, sigkill_process};
use super::background_spec::BackgroundRunSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
struct KillTargets {
    worker_pid: Option<u32>,
    agent_pid: Option<u32>,
}

pub(super) fn terminate_task_processes(worker_pid: Option<u32>, spec: &BackgroundRunSpec) {
    let targets = kill_targets(worker_pid, spec);
    #[cfg(test)]
    RECORDED_KILLS.with(|kills| kills.borrow_mut().push(targets.pids()));
    terminate_targets(&targets);
}

#[cfg(test)]
thread_local! {
    pub(super) static RECORDED_KILLS: std::cell::RefCell<Vec<Vec<u32>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(not(test))]
fn terminate_targets(targets: &KillTargets) {
    for pid in targets.pids() {
        kill_process(pid);
    }
    for pid in targets.pids() {
        sigkill_process(pid);
    }
}

#[cfg(test)]
fn terminate_targets(_targets: &KillTargets) {}

fn kill_targets(worker_pid: Option<u32>, spec: &BackgroundRunSpec) -> KillTargets {
    let agent_pid = spec.agent_pid.filter(|pid| Some(*pid) != worker_pid);
    KillTargets {
        worker_pid,
        agent_pid,
    }
}

impl KillTargets {
    fn pids(&self) -> Vec<u32> {
        [self.worker_pid, self.agent_pid]
            .into_iter()
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(agent_pid: Option<u32>) -> BackgroundRunSpec {
        BackgroundRunSpec {
            task_id: "t-kill".to_string(),
            worker_pid: Some(11),
            agent_name: "codex".to_string(),
            prompt: "prompt".to_string(),
            dir: Some(".".to_string()),
            output: None,
            result_file: None,
            result_file_required: None,
            model: None,
            budget: false,
            session_id: None,
            verify: None,
            setup: None,
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            judge: None,
            judge_retry: false,
            max_duration_mins: None,
            max_duration_secs: None,
            max_task_cost: None,
            idle_timeout_secs: None,
            retry: 0,
            group: None,
            skills: vec![],
            checklist: vec![],
            hooks: vec![],
            template: None,
            worktree: None,
            base_branch: None,
            peer_review: None,
            audit: false,
            audit_explicit: false,
            no_audit: false,
            scope: vec![],
            interactive: true,
            on_done: None,
            cascade: vec![],
            parent_task_id: None,
            env: None,
            env_forward: None,
            agent_pid,
            sandbox: false,
            read_only: false,
            audit_report_mode: false,
            container: None,
            link_deps: true,
            pre_task_dirty_paths: None,
            foreground: false,
        }
    }

    #[test]
    fn max_duration_kill_targets_include_worker_and_agent_processes() {
        let targets = kill_targets(Some(11), &spec(Some(22)));

        assert_eq!(targets.pids(), vec![11, 22]);
    }

    #[test]
    fn kill_targets_deduplicate_same_worker_and_agent_pid() {
        let targets = kill_targets(Some(11), &spec(Some(11)));

        assert_eq!(targets.pids(), vec![11]);
    }
}
