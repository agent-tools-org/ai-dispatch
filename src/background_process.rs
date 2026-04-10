// Background process helpers for aid task workers.
// Exports PID persistence updates, process lifecycle helpers, and on_done callbacks.

use anyhow::{Context, Result};
use std::process::Command;

use super::background_spec::{load_spec, load_spec_if_exists, save_spec};

pub(crate) fn update_worker_pid(task_id: &str, worker_pid: u32) -> Result<()> {
    let mut spec = load_spec(task_id)?;
    spec.worker_pid = Some(worker_pid);
    save_spec(&spec)
}

pub fn update_agent_pid(task_id: &str, agent_pid: u32) -> Result<()> {
    let mut spec = load_spec(task_id)?;
    spec.agent_pid = Some(agent_pid);
    save_spec(&spec)
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
    if pid > i32::MAX as u32 {
        return false;
    }
    let result = unsafe { libc::kill(pid as i32, 0) };
    if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EPERM) {
        return false;
    }
    is_process_not_zombie(pid)
}

#[cfg(unix)]
fn is_process_not_zombie(pid: u32) -> bool {
    let mut status = 0;
    let ret = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
    // waitpid returns:
    //   0: child exists, not yet exited -> alive
    //  >0: child was zombie, now reaped -> dead
    //  -1 ECHILD: not our child -> can't determine zombie status, trust kill(0)
    ret == 0
        || (ret == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD))
}

#[cfg(not(unix))]
pub fn is_process_running(_pid: u32) -> bool {
    false
}
