// Dispatch regression for a hung installed agy help probe.
// Exercises the production CLI adapter through an isolated aid subprocess.
// Deps: common command helpers, fake shell executable, tempfile.

mod common;
use common::aid_cmd_in;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn output_with_deadline(mut command: Command) -> Output {
    let mut child = command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("aid");
    let deadline = Instant::now() + Duration::from_secs(18);
    while child.try_wait().expect("aid status").is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("hung help blocked dispatch beyond its probe deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().expect("aid output")
}

#[test]
fn hung_agy_help_is_reaped_and_dispatch_preview_continues() {
    let home = tempfile::tempdir().expect("aid home");
    let bin = home.path().join("bin");
    fs::create_dir(&bin).expect("bin");
    let agy = bin.join("agy");
    fs::write(&agy, "#!/bin/sh\nif [ \"$1\" = --help ]; then echo $$ > \"$0.pid\"; exec sleep 60; fi\necho agy 1.1\n")
        .expect("fake agy");
    fs::set_permissions(&agy, fs::Permissions::from_mode(0o755)).expect("executable agy");
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").expect("PATH")));
    let mut command = aid_cmd_in(home.path());
    command.env("PATH", std::env::join_paths(paths).expect("probe PATH"))
        .args(["run", "agy", "Inspect the scheduler and report findings", "--read-only",
            "--dry-run", "--no-hint", "--no-skill", "--difficulty", "moderate",
            "--budget", "standard", "--urgency", "normal", "--rigor", "standard"]);
    let start = Instant::now();
    let output = output_with_deadline(command);
    let elapsed = start.elapsed();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("failed to probe agy capabilities"), "{stderr}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("[dry-run] Agent: agy"));
    assert!(elapsed >= Duration::from_secs(10) && elapsed < Duration::from_secs(15), "{elapsed:?}");
    let pid: i32 = fs::read_to_string(agy.with_extension("pid"))
        .expect("probe pid").trim().parse().expect("pid");
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "help probe still exists");
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
}
