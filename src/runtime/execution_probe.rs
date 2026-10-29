// Non-reaping process probes for stop identity checks and exit confirmation.
// Uses signal zero and ps; process owners retain responsibility for wait/reap.
use anyhow::{Context, Result, bail};

#[cfg(unix)]
pub(crate) fn alive(pid: u32) -> Result<bool> {
    if pid > i32::MAX as u32 {
        return Ok(false);
    }
    anyhow::ensure!(pid > 1, "Process identity unverifiable for pid {pid}");
    if !exists(pid)? {
        return Ok(false);
    }
    match field(pid, "stat") {
        Ok(state) => Ok(!state.starts_with('Z') && !state.starts_with('X')),
        Err(_) if !exists(pid)? => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn exists(pid: u32) -> Result<bool> {
    if unsafe { libc::kill(pid as i32, 0) } == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(error).context("Process liveness unverifiable"),
    }
}

#[cfg(not(unix))]
pub(crate) fn alive(_pid: u32) -> Result<bool> {
    bail!("Process liveness unverifiable on this platform")
}

pub(crate) fn field(pid: u32, column: &str) -> Result<String> {
    let output = std::process::Command::new("ps")
        .args(["-ww", "-p", &pid.to_string(), "-o", &format!("{column}=")])
        .output()
        .context("Process identity unreadable")?;
    anyhow::ensure!(
        output.status.success(),
        "Process identity unreadable for pid {pid}"
    );
    let value = String::from_utf8(output.stdout).context("Process identity unreadable")?;
    let value = value.trim();
    if value.is_empty() {
        bail!("Process identity unreadable for pid {pid}");
    }
    Ok(value.to_owned())
}
