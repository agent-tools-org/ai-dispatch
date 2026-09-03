// Background worker process handoff.
// Exports spawn_worker and the launcher-side double fork used by foreground runs.
// Deps: libc, the aid binary, task-id validation, and std process I/O.

use anyhow::{Context, Result};
use std::io::Read;
use std::process::{Child, Command, Stdio};

use crate::sanitize;

pub(crate) fn spawn_worker(task_id: &str) -> Result<(Child, u32)> {
    sanitize::validate_task_id(task_id)?;
    let exe = std::env::current_exe().context("Failed to resolve current aid binary")?;
    let detached = cfg!(unix)
        && !cfg!(test)
        && std::env::var_os("AID_NO_DETACH").is_none();
    let mut cmd = Command::new(exe);
    cmd.args(["__run-task", task_id])
        .stdin(Stdio::null())
        .stdout(if detached { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::null());
    if let Ok(home) = std::env::var("AID_HOME") {
        cmd.env("AID_HOME", home);
    }
    if detached {
        cmd.env("AID_DAEMONIZE_WORKER", "1");
    }
    let mut launcher = cmd.spawn().context("Failed to spawn detached background worker")?;
    if detached {
        let mut output = String::new();
        launcher
            .stdout
            .take()
            .context("Detached worker launcher did not expose its PID")?
            .read_to_string(&mut output)
            .context("Failed to read detached worker PID")?;
        let status = launcher.wait().context("Failed to reap worker launcher")?;
        if !status.success() {
            anyhow::bail!("Detached worker launcher exited with {status}");
        }
        let pid = output
            .trim()
            .parse::<u32>()
            .context("Detached worker launcher returned an invalid PID")?;
        return Ok((launcher, pid));
    }
    let pid = launcher.id();
    Ok((launcher, pid))
}

/// Convert the direct child into a reparented worker before the caller can kill its tree.
pub(crate) fn daemonize_worker_if_requested() -> Result<bool> {
    #[cfg(unix)]
    if std::env::var_os("AID_DAEMONIZE_WORKER").is_some() {
        return daemonize_worker();
    }
    Ok(false)
}

#[cfg(unix)]
fn daemonize_worker() -> Result<bool> {
    let first = unsafe { libc::fork() };
    if first < 0 {
        anyhow::bail!("Failed to fork background worker launcher");
    }
    if first > 0 {
        let mut status = 0;
        unsafe { libc::waitpid(first, &mut status, 0) };
        return Ok(true);
    }

    let worker = unsafe { libc::fork() };
    if worker < 0 {
        unsafe { libc::_exit(1) };
    }
    if worker > 0 {
        write_worker_pid(worker as u32);
        unsafe { libc::_exit(0) };
    }
    if unsafe { libc::setsid() } < 0 {
        unsafe { libc::_exit(1) };
    }
    redirect_worker_stdio()?;
    Ok(false)
}

#[cfg(unix)]
fn write_worker_pid(pid: u32) {
    let bytes = format!("{pid}\n");
    unsafe {
        libc::write(1, bytes.as_ptr().cast(), bytes.len());
    }
}

#[cfg(unix)]
fn redirect_worker_stdio() -> Result<()> {
    use std::ffi::CString;
    let path = CString::new("/dev/null").context("Invalid null device path")?;
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR) };
    if fd < 0 {
        anyhow::bail!("Failed to open null device for background worker");
    }
    for target in 0..=2 {
        if unsafe { libc::dup2(fd, target) } < 0 {
            unsafe { libc::close(fd) };
            anyhow::bail!("Failed to detach background worker stdio");
        }
    }
    if fd > 2 {
        unsafe { libc::close(fd) };
    }
    Ok(())
}
