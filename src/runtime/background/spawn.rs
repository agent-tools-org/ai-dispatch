// Background worker process handoff.
// Exports spawn_worker and the launcher-side double fork used by foreground runs.
// Deps: libc, the aid binary, task-id validation, and std process I/O.

use anyhow::{Context, Result};
use std::io::{BufRead, Read};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const HANDOFF_TIMEOUT: Duration = Duration::from_millis(500);
#[cfg(unix)]
const ACK_TIMEOUT_MS: i32 = 10_000;

#[derive(Debug)]
pub(crate) struct WorkerSpawned(pub anyhow::Error);

impl std::fmt::Display for WorkerSpawned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { std::fmt::Display::fmt(&self.0, f) }
}
impl std::error::Error for WorkerSpawned {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { Some(self.0.as_ref()) }
}

#[cfg(not(test))]
pub(crate) fn spawn_worker(task_id: &str) -> Result<impl FnOnce() -> Result<(Child, u32)>> {
    crate::support::sanitize::validate_task_id(task_id)?;
    let exe = std::env::current_exe().context("Failed to resolve current aid binary")?;
    let detached = cfg!(unix) && std::env::var_os("AID_NO_DETACH").is_none();
    let mut cmd = Command::new(exe);
    cmd.args(["__run-task", task_id])
        .stdin(if detached { Stdio::piped() } else { Stdio::null() })
        .stdout(if detached { Stdio::piped() } else { Stdio::null() })
        .stderr(worker_log(task_id)?);
    if let Ok(home) = std::env::var("AID_HOME") {
        cmd.env("AID_HOME", home);
    }
    if detached {
        cmd.env("AID_DAEMONIZE_WORKER", "1");
    }
    Ok(move || spawn_prepared(&mut cmd, detached))
}

fn spawn_prepared(cmd: &mut Command, detached: bool) -> Result<(Child, u32)> {
    let mut launcher = cmd.spawn().context("Failed to spawn detached background worker")?;
    if !detached {
        let pid = launcher.id();
        return Ok((launcher, pid));
    }
    let result = receive_worker_pid(&mut launcher);
    match result {
        Ok(pid) => Ok((launcher, pid)),
        Err(error) => {
            launcher.stdin.take();
            let _ = launcher.kill();
            std::thread::spawn(move || {
                let _ = launcher.wait();
            });
            Err(WorkerSpawned(error).into())
        }
    }
}

fn receive_worker_pid(launcher: &mut Child) -> Result<u32> {
    let stdout = launcher.stdout.take().context("Worker launcher did not expose its PID")?;
    let (sent, received) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout.take(12));
        let mut line = String::new();
        let result = reader
            .read_line(&mut line)
            .context("Failed to read detached worker PID")
            .and_then(|_| parse_worker_pid(&line));
        let _ = sent.send((result, reader.into_inner().into_inner()));
    });
    let (pid, stdout) = received.recv_timeout(HANDOFF_TIMEOUT).context("Worker PID handoff timed out")?;
    launcher.stdout = Some(stdout);
    pid
}

fn parse_worker_pid(line: &str) -> Result<u32> {
    anyhow::ensure!(line.ends_with('\n'), "Incomplete detached worker PID");
    let pid = line.trim().parse::<u32>().context("Invalid detached worker PID")?;
    anyhow::ensure!(pid > 1 && pid <= i32::MAX as u32, "Invalid detached worker PID");
    Ok(pid)
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
    if !worker_acknowledged() {
        unsafe { libc::_exit(1) };
    }
    if unsafe { libc::setsid() } < 0 {
        unsafe { libc::_exit(1) };
    }
    redirect_worker_stdio()?;
    Ok(false)
}

#[cfg(unix)]
fn worker_acknowledged() -> bool {
    let mut fd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
    if unsafe { libc::poll(&mut fd, 1, ACK_TIMEOUT_MS) } <= 0 {
        return false;
    }
    let mut ack = [0u8];
    unsafe { libc::read(0, ack.as_mut_ptr().cast(), 1) == 1 && ack[0] == b'1' }
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
    for target in 0..=1 {
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

fn worker_log(task_id: &str) -> Result<std::fs::File> {
    crate::support::sanitize::validate_task_id(task_id)?;
    let dir = crate::support::paths::task_dir(task_id);
    std::fs::create_dir_all(&dir)?;
    std::fs::OpenOptions::new().create(true).append(true).open(dir.join("worker.log"))
        .context("Failed to open worker log")
}

#[cfg(test)]
impl super::spec::BackgroundRunSpec {
    pub(crate) fn test_worker_handoff(
        cmd: &mut Command,
        inside: impl FnOnce(),
    ) -> Result<(Result<u32>, Child)> {
        let mut launcher = cmd.spawn()?;
        inside();
        let result = receive_worker_pid(&mut launcher);
        Ok((result, launcher))
    }
}

#[cfg(test)]
#[path = "spawn_tests.rs"]
mod tests;
