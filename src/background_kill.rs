// Shared reaper process termination helpers.
// Exports task-process termination; a stored worker pid is signalled only after
// its `__run-task <id>` command line proves it is still this task's worker.
// Deps: background process signals, run specs, process_group and `ps` on Unix.

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
    #[cfg(not(test))]
    signal_targets(&targets, &spec.task_id);
}

#[cfg(test)]
thread_local! {
    pub(super) static RECORDED_KILLS: std::cell::RefCell<Vec<Vec<u32>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The stored worker pid is signalled, and its tree walked, only while it still
/// runs `__run-task <task_id>`: a reused pid may belong to any process, even a
/// root one whose user-owned children an unchecked walk would kill. The worker's
/// descendants are captured before any signal: the agent's tool commands run in
/// their own sessions and are untraceable once the worker and agent die.
#[cfg_attr(test, allow(dead_code))]
fn signal_targets(targets: &KillTargets, task_id: &str) {
    let worker_pid = targets.worker_pid.filter(|pid| is_task_worker(*pid, task_id));
    let mut pids = [worker_pid, targets.agent_pid].into_iter().flatten().collect::<Vec<_>>();
    #[cfg(unix)]
    if let Some(worker_pid) = worker_pid {
        let descendants = crate::process_group::descendant_pids(worker_pid);
        pids.extend(descendants.into_iter().filter(|pid| Some(*pid) != targets.agent_pid));
    }
    for &pid in &pids {
        kill_process(pid);
    }
    for &pid in &pids {
        sigkill_process(pid);
    }
}

#[cfg(unix)]
fn is_task_worker(pid: u32, task_id: &str) -> bool {
    std::process::Command::new("ps")
        .args(["-o", "args=", "-p", &pid.to_string()])
        .output()
        .is_ok_and(|out| out.status.success() && names_worker(&String::from_utf8_lossy(&out.stdout), task_id))
}

#[cfg(not(unix))]
fn is_task_worker(_pid: u32, _task_id: &str) -> bool {
    false
}

/// Whether a command line is `... __run-task <task_id> ...` (the spawn_worker argv).
fn names_worker(args: &str, task_id: &str) -> bool {
    let tokens = args.split_whitespace().collect::<Vec<_>>();
    tokens.windows(2).any(|pair| pair[0] == "__run-task" && pair[1] == task_id)
}

fn kill_targets(worker_pid: Option<u32>, spec: &BackgroundRunSpec) -> KillTargets {
    let agent_pid = spec.agent_pid.filter(|pid| Some(*pid) != worker_pid);
    KillTargets {
        worker_pid,
        agent_pid,
    }
}

#[cfg(test)]
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
    #[cfg(unix)]
    use crate::test_subprocess::is_live;

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

    #[test]
    fn worker_identity_needs_the_run_task_and_task_id_pair() {
        assert!(names_worker("/Users/x/My Apps/aid __run-task t-kill", "t-kill"));
        assert!(!names_worker("aid __run-task t-kill2", "t-kill"));
        assert!(!names_worker("aid t-kill __run-task", "t-kill"));
        assert!(!names_worker("-zsh", "t-kill"));
        assert!(!names_worker("", "t-kill"));
    }

    /// A stale worker pid reused by an unrelated process: neither that process nor
    /// its child (in its own group, as Claude's Bash commands are) may be signalled.
    #[cfg(unix)]
    #[test]
    fn reused_worker_pid_leaves_unrelated_process_and_child_alive() {
        let _permit = crate::test_subprocess::acquire();
        let (mut parent, child) = spawn_tree(&["sh"]);
        signal_targets(&KillTargets { worker_pid: Some(parent.id()), agent_pid: None }, "t-kill");
        std::thread::sleep(std::time::Duration::from_millis(300));

        let survived = parent.try_wait().ok().flatten().is_none() && is_live(child);
        unsafe { libc::kill(child, libc::SIGKILL) };
        let _ = parent.kill();
        let _ = parent.wait();
        assert!(survived, "unverified worker pid must not be signalled or walked");
    }

    #[cfg(unix)]
    #[test]
    fn verified_worker_and_its_descendant_are_killed() {
        let _permit = crate::test_subprocess::acquire();
        let (mut parent, child) = spawn_tree(&["__run-task", "t-kill"]);
        signal_targets(&KillTargets { worker_pid: Some(parent.id()), agent_pid: None }, "t-kill");

        let parent_killed = parent.wait().is_ok_and(|status| !status.success());
        let child_gone = (0..100).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            !is_live(child)
        });
        if !child_gone {
            unsafe { libc::kill(child, libc::SIGKILL) };
        }
        assert!(parent_killed && child_gone, "verified worker tree must be killed");
    }

    /// `sh -c <script> <argv...>` whose child leads its own process group.
    #[cfg(unix)]
    fn spawn_tree(argv: &[&str]) -> (std::process::Child, i32) {
        use std::os::unix::process::CommandExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let pid_file = dir.path().join("child.pid");
        let script = "perl -e 'setpgrp(0, 0); exec q(sleep), q(60)' & echo $! > \"$PID_FILE\"; wait";
        let mut parent = std::process::Command::new("/bin/sh")
            .arg("-c").arg(script).args(argv).env("PID_FILE", &pid_file).process_group(0)
            .spawn().expect("process tree should spawn");
        for _ in 0..200 {
            if let Some(pid) = std::fs::read_to_string(&pid_file).ok()
                .and_then(|text| text.trim().parse::<i32>().ok())
                .filter(|pid| unsafe { libc::getpgid(*pid) } == *pid)
            {
                return (parent, pid);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = parent.kill();
        panic!("child must start and lead its own process group");
    }
}
