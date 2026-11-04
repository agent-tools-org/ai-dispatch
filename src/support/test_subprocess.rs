// Test-only subprocess semaphore for limiting parallel child-process spawns.
// Exports: acquire() permit guard for subprocess-heavy tests, is_live for pid checks, and
// wait_for_exec so identity probes never read a child that is still inside exec.
// Deps: std::sync::{Condvar, Mutex, OnceLock}

use std::sync::{Condvar, Mutex, OnceLock};

const MAX_TEST_SUBPROCESSES: usize = 8;

struct Semaphore {
    count: Mutex<usize>,
    condvar: Condvar,
}

pub struct SubprocessPermit;

fn semaphore() -> &'static Semaphore {
    static SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| Semaphore {
        count: Mutex::new(0),
        condvar: Condvar::new(),
    })
}

pub fn acquire() -> SubprocessPermit {
    let semaphore = semaphore();
    let mut count = semaphore.count.lock().unwrap();
    while *count >= MAX_TEST_SUBPROCESSES {
        count = semaphore.condvar.wait(count).unwrap();
    }
    *count += 1;
    SubprocessPermit
}

impl Drop for SubprocessPermit {
    fn drop(&mut self) {
        let semaphore = semaphore();
        let mut count = semaphore.count.lock().unwrap();
        *count -= 1;
        semaphore.condvar.notify_one();
    }
}

/// `spawn` can return while the child is inside exec: Linux wakes the vfork parent before it
/// switches the child's address space, so /proc/<pid>/cmdline first shows the parent's
/// arguments and then nothing. A fixture that probes identity in that window reads a live
/// worker as gone.
#[cfg(target_os = "linux")]
pub fn wait_for_exec(pid: u32) {
    let parent = std::fs::read("/proc/self/cmdline").unwrap_or_default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match std::fs::read(format!("/proc/{pid}/cmdline")) {
            Ok(args) if args == parent => {}
            Ok(args) if args.is_empty() => {
                // An exited child also has empty arguments and will never finish exec.
                if !crate::runtime::execution_probe::alive(pid).unwrap_or(false) {
                    return;
                }
            }
            _ => return,
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Signal 0 succeeds on a zombie that an init without reaping keeps around.
#[cfg(unix)]
pub fn is_live(pid: i32) -> bool {
    if unsafe { libc::kill(pid, 0) } != 0 {
        return false;
    }
    let stat = std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output();
    stat.map(|out| !String::from_utf8_lossy(&out.stdout).trim_start().starts_with('Z'))
        .unwrap_or(true)
}

/// Signals are asynchronous: wait up to `timeout` for `pid` to stop being live.
#[cfg(unix)]
pub fn wait_dead(pid: i32, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while is_live(pid) {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    true
}
