// Bounded subprocess capture for caller-owned CLI acceptance tests.
// Uses file-backed output so detached workers cannot hold a capture pipe open.
// Deps: std process/files/time, tempfile and libc.

use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

pub fn spawn(cmd: &mut Command) -> (Child, std::fs::File, std::fs::File) {
    let stdout = tempfile::tempfile().unwrap();
    let stderr = tempfile::tempfile().unwrap();
    // Bound the CLI process group too, without pipe EOF depending on detached workers.
    use std::os::unix::process::CommandExt;
    cmd.process_group(0)
        .stdout(stdout.try_clone().unwrap())
        .stderr(stderr.try_clone().unwrap());
    (cmd.spawn().unwrap(), stdout, stderr)
}

pub fn finish(mut child: Child, mut stdout: std::fs::File, mut stderr: std::fs::File) -> Output {
    use std::io::{Read, Seek, SeekFrom};
    let deadline = Instant::now() + Duration::from_secs(35);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            panic!("CLI exceeded 35 second deadline");
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let mut out = Output {
        status,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    stdout.seek(SeekFrom::Start(0)).unwrap();
    stderr.seek(SeekFrom::Start(0)).unwrap();
    stdout.read_to_end(&mut out.stdout).unwrap();
    stderr.read_to_end(&mut out.stderr).unwrap();
    out
}

pub fn bounded(cmd: &mut Command) -> Output {
    let (child, stdout, stderr) = spawn(cmd);
    finish(child, stdout, stderr)
}
