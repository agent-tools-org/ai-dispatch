// Shared reaper process termination helpers.
// Exports termination after worker command-line and persisted agent launch checks.
// Deps: background process signals, run specs, process_group and `ps` on Unix.

use super::process::{kill_process, sigkill_process};
use super::spec::BackgroundRunSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
struct KillTargets {
    worker_pid: Option<u32>,
    agent_pid: Option<u32>,
}

pub(super) fn terminate_task_processes(worker_pid: Option<u32>, spec: &BackgroundRunSpec) {
    #[cfg(test)]
    RECORDED_KILLS.with(|kills| kills.borrow_mut().push(kill_targets(worker_pid, spec).pids()));
    #[cfg(not(test))]
    signal_targets(worker_pid, spec);
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
fn signal_targets(worker_pid: Option<u32>, spec: &BackgroundRunSpec) {
    let worker_pid = worker_pid.filter(|pid| {
        Some(*pid) != spec.agent_pid && BackgroundRunSpec::is_task_worker(*pid, &spec.task_id)
    });
    let mut pids = worker_pid.into_iter().collect::<Vec<_>>();
    pids.extend(capture_descendants(worker_pid, spec, false).into_iter().map(|(pid, _)| pid));
    for signal in [kill_process, sigkill_process] {
        for &pid in &pids {
            signal(pid);
        }
        if let Some(pid) = spec.agent_pid_for_signal() {
            signal(pid);
        }
    }
}

pub(crate) fn capture_descendants(worker: Option<u32>, spec: &BackgroundRunSpec, agent_children: bool)
    -> Vec<(u32, Option<String>)> {
    let agent = spec.agent_pid_for_signal();
    let worker = worker.filter(|pid| BackgroundRunSpec::is_task_worker(*pid, &spec.task_id))
        .filter(|_| spec.agent_pid.is_none() || agent.is_some());
    let mut roots = worker.into_iter().collect::<Vec<_>>();
    if agent_children { roots.extend(agent); }
    let mut found = Vec::new();
    #[cfg(unix)]
    for root in roots {
        for pid in crate::runtime::process::group::descendant_pids(root) {
            if Some(pid) != spec.agent_pid && !found.iter().any(|(seen, _)| *seen == pid) {
                found.push((pid, crate::runtime::process::monitor::activity::root_started(pid)));
            }
        }
    }
    found
}

pub(crate) fn signal_descendants(targets: &[(u32, Option<String>)], force: bool) {
    for (pid, started) in targets {
        if crate::runtime::process::monitor::activity::root_matches(*pid, started.as_deref()) {
            if force { sigkill_process(*pid); } else { kill_process(*pid); }
        }
    }
}

impl BackgroundRunSpec {
    #[cfg(unix)]
    pub(crate) fn is_task_worker(pid: u32, task_id: &str) -> bool {
        std::process::Command::new("ps")
            .args(["-o", "args=", "-p", &pid.to_string()])
            .output()
            .is_ok_and(|out| {
                out.status.success() && names_worker(&String::from_utf8_lossy(&out.stdout), task_id)
            })
    }

    #[cfg(not(unix))]
    pub(crate) fn is_task_worker(_pid: u32, _task_id: &str) -> bool {
        false
    }
}

/// Whether a command line is `... __run-task <task_id> ...` (the spawn_worker argv).
fn names_worker(args: &str, task_id: &str) -> bool {
    let tokens = args.split_whitespace().collect::<Vec<_>>();
    tokens.windows(2).any(|pair| pair[0] == "__run-task" && pair[1] == task_id)
}

#[cfg(test)]
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
    use crate::support::test_subprocess::is_live;

    fn spec(agent_pid: Option<u32>) -> BackgroundRunSpec {
        BackgroundRunSpec {
            task_id: "t-kill".to_string(),
            worker_pid: Some(11),
            dir: Some(".".to_string()),
            interactive: true,
            agent_pid,
            ..Default::default()
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

    #[cfg(unix)]
    #[test]
    fn detached_kill_rejects_reused_or_missing_agent_identity() {
        let _permit = crate::support::test_subprocess::acquire();
        for started in [Some("previous-launch".to_string()), None] {
            let (mut parent, child) = spawn_tree(&["sh"]);
            let mut spec = spec(Some(parent.id()));
            spec.agent_started = started;
            let targets = kill_targets(None, &spec);
            signal_targets(targets.worker_pid, &spec);
            std::thread::sleep(std::time::Duration::from_millis(100));
            let survived = parent.try_wait().expect("wait").is_none() && is_live(child);
            unsafe { libc::kill(child, libc::SIGKILL) };
            let _ = parent.kill();
            let _ = parent.wait();
            assert!(survived, "unverified agent root and child must survive settlement");
            assert!(spec.agent_pid_for_signal().is_none(), "unverified agent must not be signalled");
        }
    }

    #[cfg(unix)]
    #[test]
    fn detached_timeout_kills_agent_with_matching_launch_identity() {
        let _permit = crate::support::test_subprocess::acquire();
        let mut command = std::process::Command::new("sleep");
        command.arg("60");
        let mut root = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("agent");
        let mut spec = spec(Some(root.id()));
        spec.agent_started = crate::runtime::process::monitor::activity::root_started(root.id());
        assert!(spec.agent_started.is_some());
        let targets = kill_targets(None, &spec);
        assert_eq!(targets.pids(), vec![root.id()]);
        signal_targets(targets.worker_pid, &spec);
        assert!(!root.child_mut().wait().expect("wait").success(), "owned agent must be killed");
    }

    #[cfg(unix)]
    #[test]
    fn detached_kill_rechecks_identity_after_target_selection() {
        let _permit = crate::support::test_subprocess::acquire();
        let mut command = std::process::Command::new("sleep");
        command.arg("60");
        let mut root = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("agent");
        let mut spec = spec(Some(root.id()));
        spec.agent_started = crate::runtime::process::monitor::activity::root_started(root.id());
        assert!(spec.agent_started.is_some());
        let targets = kill_targets(None, &spec);
        spec.agent_started = Some("previous-launch".to_string());
        signal_targets(targets.worker_pid, &spec);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let survived = root.child_mut().try_wait().expect("wait").is_none();
        root.force_kill();
        assert!(survived, "target selection must not authorize a later mismatched root");
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
        let _permit = crate::support::test_subprocess::acquire();
        let (mut parent, child) = spawn_tree(&["sh"]);
        signal_targets(Some(parent.id()), &spec(None));
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
        let _permit = crate::support::test_subprocess::acquire();
        let (mut parent, child) = spawn_tree(&["__run-task", "t-kill"]);
        signal_targets(Some(parent.id()), &spec(None));

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
