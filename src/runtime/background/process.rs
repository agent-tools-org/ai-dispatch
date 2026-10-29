// Background process helpers for aid task workers.
// Exports PID persistence updates, process lifecycle helpers, and on_done callbacks.

use anyhow::{Context, Result};
use std::process::Command;

use super::spec::{load_spec, load_spec_if_exists, save_spec};

pub(crate) fn update_worker_pid(task_id: &str, worker_pid: u32) -> Result<()> {
    let mut spec = load_spec(task_id)?;
    spec.worker_pid = Some(worker_pid);
    save_spec(&spec)
}

pub fn update_agent_pid(task_id: &str, agent_pid: u32) -> Result<()> {
    let result = (|| {
        let mut spec = load_spec(task_id)?;
        spec.agent_pid = Some(agent_pid);
        spec.agent_started = crate::runtime::process::monitor::activity::root_started(agent_pid);
        save_spec(&spec)
    })();
    if result.is_err() {
        sigkill_process(agent_pid);
    }
    result
}

pub fn load_agent_pid(task_id: &str) -> Result<Option<u32>> {
    Ok(load_spec_if_exists(task_id)?.and_then(|spec| spec.agent_pid))
}

pub(crate) fn spawn_on_done_command(command: &str, task_id: &str, status: &str) -> Result<()> {
    let mut cmd = build_on_done_command(command)?;
    cmd.env("AID_TASK_ID", task_id)
        .env("AID_TASK_STATUS", status);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    // Reap the child in a background thread to prevent orphan/zombie processes.
    let child = cmd.spawn().context("failed to spawn on_done callback")?;
    #[cfg(test)]
    ON_DONE_SPAWN.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() { hook(); }
    });
    let command_name = command.to_string();
    std::thread::spawn(move || match child.wait_with_output() {
        Ok(output) if !output.status.success() => {
            aid_error!("[aid] on_done callback failed: {command_name}");
        }
        Err(err) => aid_error!("[aid] on_done callback wait failed: {err}"),
        _ => {}
    });
    Ok(())
}

pub(crate) fn build_on_done_command(command: &str) -> Result<Command> {
    if contains_shell_metacharacters(command) {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);
        return Ok(cmd);
    }
    let mut parts = command.split_whitespace();
    let program = parts.next().context("on_done command is empty")?;
    let args: Vec<&str> = parts.collect();
    let mut cmd = Command::new(program);
    cmd.args(&args);
    Ok(cmd)
}

fn contains_shell_metacharacters(command: &str) -> bool {
    ["&&", "||", "|", ";", ">", "<", "`", "$("]
        .iter()
        .any(|pattern| command.contains(pattern))
}

#[cfg(unix)]
pub fn kill_process(pid: u32) {
    if pid > i32::MAX as u32 {
        return;
    }
    let pid_i32 = pid as i32;
    unsafe {
        libc::kill(-pid_i32, libc::SIGTERM);
        libc::kill(pid_i32, libc::SIGTERM);
    }
}

#[cfg(not(unix))]
pub fn kill_process(_pid: u32) {}

#[cfg(unix)]
pub fn sigkill_process(pid: u32) {
    if pid > i32::MAX as u32 {
        return;
    }
    let pid_i32 = pid as i32;
    unsafe {
        libc::kill(-pid_i32, libc::SIGKILL);
        libc::kill(pid_i32, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
pub fn sigkill_process(_pid: u32) {}

#[cfg(unix)]
pub fn is_process_running(pid: u32) -> bool {
    crate::runtime::execution_processes::probe::alive(pid).unwrap_or(true)
}

#[cfg(not(unix))]
pub fn is_process_running(_pid: u32) -> bool {
    false
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    #[test]
    fn agent_pid_persistence_failure_kills_spawned_group() {
        let _permit = crate::support::test_subprocess::acquire();
        let _home = crate::support::test_env::TestHome::new();
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("agent");
        let pid = child.id();
        assert!(update_agent_pid("t-missing-spec", pid).is_err());
        assert!(!child.wait().expect("reap failed launch").success());
        assert!(!is_process_running(pid));
    }
}

#[cfg(test)]
thread_local! {
    pub(super) static ON_DONE_SPAWN: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, unix))]
#[test]
fn caller_owned_liveness_probe_does_not_reap_a_recorded_child() {
    let _permit = crate::support::test_subprocess::acquire();
    let mut child = std::process::Command::new("sh").args(["-c", "exit 7"]).spawn().expect("child");
    assert!(crate::support::test_subprocess::wait_dead(child.id() as i32, std::time::Duration::from_secs(5)));
    assert!(!is_process_running(child.id()));
    assert_eq!(child.wait().expect("owner still owns wait").code(), Some(7));
}
