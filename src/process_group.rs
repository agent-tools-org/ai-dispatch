// Process-group termination helpers shared by agent runners and watchers.
// Exports cleanup_process_group, force_kill_process_group, kill_with_grace, kill_group_with_grace
// and descendant_pids; descendants are re-verified by start time before the SIGKILL pass.
// Deps: tokio::process::Child, libc and `ps` on Unix.

#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
const DEFAULT_KILL_GRACE: Duration = Duration::from_secs(3);
#[cfg(unix)]
const KILL_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[cfg(unix)]
pub fn cleanup_process_group(child: &tokio::process::Child) {
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
    }
}

#[cfg(not(unix))]
pub fn cleanup_process_group(_child: &tokio::process::Child) {}

#[cfg(unix)]
pub fn force_kill_process_group(child: &tokio::process::Child) {
    if let Some(pid) = child.id() {
        kill_with_grace(pid as i32, DEFAULT_KILL_GRACE);
    }
}

#[cfg(not(unix))]
pub fn force_kill_process_group(_child: &tokio::process::Child) {}

/// Signals the group led by `pgid` and every descendant of its leader, captured
/// before the first signal: an agent's tool commands (Claude Code's Bash) run in
/// their own session, so the group alone misses them, and once the leader dies
/// they are reparented to init and no longer traceable to the task.
/// The caller must still own the unreaped leader, so `pgid` cannot be reused.
#[cfg(unix)]
pub fn kill_with_grace(pgid: i32, grace: Duration) {
    if pgid <= 0 {
        return;
    }
    kill_group_and(pgid, &descendants(pgid as u32), grace);
}

/// Signals only the group: for a leader that is already reaped, whose pid no
/// longer identifies the task and must not be expanded into a process tree.
#[cfg(unix)]
pub fn kill_group_with_grace(pgid: i32, grace: Duration) {
    if pgid > 0 {
        kill_group_and(pgid, &[], grace);
    }
}

#[cfg(unix)]
fn kill_group_and(pgid: i32, descendants: &[ProcessEntry], grace: Duration) {
    let pids = descendants.iter().map(|entry| entry.pid).collect::<Vec<_>>();
    signal_process_group(pgid, libc::SIGTERM);
    signal_processes(&pids, libc::SIGTERM);
    wait_for_process_group_exit(pgid, grace);
    if process_group_exists(pgid) {
        signal_process_group(pgid, libc::SIGKILL);
    }
    signal_processes(&unchanged_pids(descendants, &process_table()), libc::SIGKILL);
}

/// Signals each pid and the group it leads, if any. Callers pass pids that are
/// live descendants (or verified unchanged), so a group with the same id can only
/// be one that the descendant created.
#[cfg(unix)]
fn signal_processes(pids: &[u32], signal: i32) {
    for &pid in pids {
        if pid > 1 && pid <= i32::MAX as u32 {
            unsafe {
                libc::kill(-(pid as i32), signal);
                libc::kill(pid as i32, signal);
            }
        }
    }
}

/// One row of a `ps` snapshot; `started` (lstart) tells a live process from a
/// later one that reused its pid.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessEntry {
    pid: u32,
    ppid: u32,
    started: String,
}

/// Every live descendant pid of `root`, from one `ps` snapshot. Empty for
/// init/0 or when `ps` is unavailable; callers still signal their own group.
#[cfg(unix)]
pub fn descendant_pids(root: u32) -> Vec<u32> {
    descendants(root).into_iter().map(|entry| entry.pid).collect()
}

#[cfg(unix)]
fn descendants(root: u32) -> Vec<ProcessEntry> {
    if root <= 1 {
        return Vec::new();
    }
    descendants_in_table(root, &process_table())
}

#[cfg(unix)]
fn process_table() -> Vec<ProcessEntry> {
    std::process::Command::new("ps")
        .args(["-A", "-o", "pid=", "-o", "ppid=", "-o", "lstart="])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_table(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

#[cfg(unix)]
fn parse_table(table: &str) -> Vec<ProcessEntry> {
    table
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let ppid = fields.next()?.parse::<u32>().ok()?;
            let started = fields.collect::<Vec<_>>().join(" ");
            (!started.is_empty()).then_some(ProcessEntry { pid, ppid, started })
        })
        .collect()
}

#[cfg(unix)]
fn descendants_in_table(root: u32, table: &[ProcessEntry]) -> Vec<ProcessEntry> {
    let mut found: Vec<ProcessEntry> = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for entry in table {
            let fresh = !found.iter().any(|seen| seen.pid == entry.pid);
            if entry.ppid == parent && entry.pid != root && entry.pid > 1 && fresh {
                found.push(entry.clone());
                frontier.push(entry.pid);
            }
        }
    }
    found
}

/// Snapshot pids still owned by the same process (same start time) in `now`.
/// An empty `now` (no `ps`) yields nothing: unverified pids are never signalled.
#[cfg(unix)]
fn unchanged_pids(snapshot: &[ProcessEntry], now: &[ProcessEntry]) -> Vec<u32> {
    snapshot
        .iter()
        .filter(|old| now.iter().any(|cur| cur.pid == old.pid && cur.started == old.started))
        .map(|old| old.pid)
        .collect()
}

#[cfg(unix)]
fn wait_for_process_group_exit(pgid: i32, grace: Duration) {
    let start = Instant::now();
    while start.elapsed() < grace {
        if !process_group_exists(pgid) {
            return;
        }
        std::thread::sleep(KILL_POLL_INTERVAL.min(grace.saturating_sub(start.elapsed())));
    }
}

#[cfg(unix)]
fn process_group_exists(pgid: i32) -> bool {
    let result = unsafe { libc::kill(-pgid, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(unix)]
fn signal_process_group(pgid: i32, signal: i32) {
    unsafe {
        libc::kill(-pgid, signal);
    }
}

#[cfg(test)]
#[cfg(unix)]
#[path = "process_group_tests.rs"]
mod tests;
