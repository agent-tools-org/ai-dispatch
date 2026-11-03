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
    signal_targets_with(worker_pid, spec, |_| {});
}

fn signal_targets_with(
    worker_pid: Option<u32>,
    spec: &BackgroundRunSpec,
    before_signals: impl FnOnce(&mut Vec<(u32, Option<String>)>),
) {
    let worker = worker_pid
        .filter(|pid| Some(*pid) != spec.agent_pid)
        .and_then(|pid| {
            let started = crate::runtime::process::monitor::activity::root_started(pid)?;
            BackgroundRunSpec::is_task_worker(pid, &spec.task_id).then_some((pid, Some(started)))
        });
    let mut targets = worker.clone().into_iter().collect::<Vec<_>>();
    targets.extend(capture_descendants(worker.map(|(pid, _)| pid), spec, true));
    before_signals(&mut targets);
    for force in [false, true] {
        signal_descendants(&targets, force);
        if let Some(pid) = spec.agent_pid_for_signal() {
            if force {
                sigkill_process(pid);
            } else {
                kill_process(pid);
            }
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
#[path = "kill_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "kill_identity_tests.rs"]
mod identity_tests;
