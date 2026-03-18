// RAII subprocess guard: spawn with process group isolation, auto-cleanup on Drop.
// All subprocess spawning should go through ProcessGuard to ensure consistent
// process group isolation, timeout enforcement, and cleanup.

use anyhow::{Context, Result};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

/// RAII guard for a subprocess spawned in its own process group.
/// On Drop, sends SIGTERM to the entire process group to clean up orphans.
pub struct ProcessGuard {
    child: Child,
    killed: bool,
}

impl ProcessGuard {
    /// Spawn a command in its own process group.
    /// The command's stdin/stdout/stderr should be configured before calling this.
    pub fn spawn(cmd: &mut Command) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let child = cmd.spawn().context("failed to spawn subprocess")?;
        Ok(Self {
            child,
            killed: false,
        })
    }

    /// Spawn a detached command (stdin/stdout/stderr → null) in its own process group.
    pub fn spawn_detached(cmd: &mut Command) -> Result<Self> {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Self::spawn(cmd)
    }

    /// Get the child's PID.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Get a mutable reference to the underlying Child.
    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Wait for the child to exit (blocks).
    /// After the child is reaped, SIGTERMs the process group to clean up grandchildren.
    pub fn wait(&mut self) -> Result<ExitStatus> {
        let status = self.child.wait().context("failed to wait for subprocess")?;
        self.sigterm_group();
        self.killed = true;
        Ok(status)
    }

    /// Wait with a timeout. Returns None if timed out (child is killed).
    /// On normal exit, SIGTERMs the process group. On timeout, SIGKILLs it.
    pub fn wait_with_timeout(&mut self, timeout: Duration) -> Result<Option<ExitStatus>> {
        let pid = self.child.id();
        let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired2 = fired.clone();
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done2 = done.clone();

        std::thread::spawn(move || {
            std::thread::sleep(timeout);
            if !done2.load(std::sync::atomic::Ordering::SeqCst) {
                #[cfg(unix)]
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
                fired2.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });

        let status = self.child.wait().context("failed to wait for subprocess")?;
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        self.sigterm_group(); // clean up orphaned grandchildren
        self.killed = true;

        if fired.load(std::sync::atomic::Ordering::SeqCst) {
            Ok(None)
        } else {
            Ok(Some(status))
        }
    }

    /// SIGTERM the process group (clean up orphaned grandchildren).
    fn sigterm_group(&self) {
        #[cfg(unix)]
        {
            let pid = self.child.id() as i32;
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
        }
    }

    /// Kill the process group immediately (SIGKILL).
    pub fn force_kill(&mut self) {
        #[cfg(unix)]
        {
            let pid = self.child.id() as i32;
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
        let _ = self.child.kill();
        self.killed = true;
    }

    /// Consume the guard without running Drop cleanup.
    /// Use when you need to transfer ownership of the child process.
    pub fn into_child(mut self) -> Child {
        self.killed = true; // prevent Drop cleanup
        std::mem::replace(
            &mut self.child,
            // Dummy — never used because killed=true prevents Drop from touching child
            Command::new("true")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("failed to spawn dummy"),
        )
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if self.killed {
            return;
        }
        // SIGTERM the entire process group to clean up any orphaned children
        #[cfg(unix)]
        {
            let pid = self.child.id() as i32;
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_and_wait() {
        let _permit = crate::test_subprocess::acquire();
        let mut cmd = Command::new("echo");
        cmd.arg("hello");
        let mut guard = ProcessGuard::spawn(&mut cmd).unwrap();
        let status = guard.wait().unwrap();
        assert!(status.success());
    }

    #[test]
    fn spawn_detached() {
        let _permit = crate::test_subprocess::acquire();
        let mut cmd = Command::new("echo");
        cmd.arg("hello");
        let mut guard = ProcessGuard::spawn_detached(&mut cmd).unwrap();
        let status = guard.wait().unwrap();
        assert!(status.success());
    }

    #[test]
    fn wait_with_timeout_completes() {
        let _permit = crate::test_subprocess::acquire();
        let mut cmd = Command::new("echo");
        cmd.arg("hello");
        let mut guard = ProcessGuard::spawn(&mut cmd).unwrap();
        let status = guard
            .wait_with_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(status.is_some());
        assert!(status.unwrap().success());
    }

    #[cfg(unix)]
    #[test]
    fn wait_with_timeout_kills_on_expiry() {
        let _permit = crate::test_subprocess::acquire();
        let mut cmd = Command::new("sleep");
        cmd.arg("60");
        let mut guard = ProcessGuard::spawn(&mut cmd).unwrap();
        let status = guard
            .wait_with_timeout(Duration::from_millis(100))
            .unwrap();
        assert!(status.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn drop_cleans_up_process_group() {
        let _permit = crate::test_subprocess::acquire();
        let mut cmd = Command::new("sleep");
        cmd.arg("60");
        let guard = ProcessGuard::spawn(&mut cmd).unwrap();
        let pid = guard.id() as i32;
        drop(guard);
        // Process should be dead after drop
        std::thread::sleep(Duration::from_millis(100));
        let result = unsafe { libc::kill(pid, 0) };
        assert_eq!(result, -1);
    }
}
