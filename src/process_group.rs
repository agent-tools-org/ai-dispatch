// Process-group termination helpers shared by agent runners and watchers.
// Exports cleanup_process_group and force_kill_process_group.
// Deps: tokio::process::Child and libc on Unix.

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
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
pub fn force_kill_process_group(_child: &tokio::process::Child) {}
