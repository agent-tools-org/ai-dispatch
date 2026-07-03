// Process-group termination helpers shared by agent runners and watchers.
// Exports cleanup_process_group, force_kill_process_group, and kill_with_grace.
// Deps: tokio::process::Child and libc on Unix.

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

#[cfg(unix)]
pub fn kill_with_grace(pgid: i32, grace: Duration) {
    if pgid <= 0 {
        return;
    }
    signal_process_group(pgid, libc::SIGTERM);
    wait_for_process_group_exit(pgid, grace);
    if process_group_exists(pgid) {
        signal_process_group(pgid, libc::SIGKILL);
    }
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
mod tests {
    use super::kill_with_grace;
    use crate::test_subprocess;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::Command;
    use std::time::Duration;

    #[test]
    fn kill_with_grace_escalates_for_term_ignoring_child() {
        let _permit = test_subprocess::acquire();
        let ready = tempfile::NamedTempFile::new().expect("ready file should be created");
        let ready_path = ready.path().to_path_buf();
        drop(ready);
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("trap '' TERM; touch \"$1\"; while :; do sleep 1; done")
            .arg("sh")
            .arg(&ready_path)
            .process_group(0);
        let mut child = command.spawn().expect("TERM-ignoring child should spawn");
        let pgid = child.id() as i32;
        for _ in 0..100 {
            if ready_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready_path.exists());

        kill_with_grace(pgid, Duration::from_millis(100));

        let status = child.wait().expect("TERM-ignoring child should exit");
        assert_eq!(status.signal(), Some(libc::SIGKILL));
    }
}
