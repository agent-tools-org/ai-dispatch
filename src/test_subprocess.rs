// Test-only subprocess semaphore for limiting parallel child-process spawns.
// Exports: acquire() permit guard for subprocess-heavy tests, and is_live for pid checks.
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
