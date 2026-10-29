// Non-reaping process probes for stop identity checks and exit confirmation.
// Uses signal zero and ps; process owners retain responsibility for wait/reap.
use anyhow::{Context, Result, bail};

#[cfg(all(unix, not(target_os = "linux")))]
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

#[cfg(target_os = "linux")]
pub(crate) fn alive(pid: u32) -> Result<bool> {
    if pid > i32::MAX as u32 { return Ok(false); }
    anyhow::ensure!(pid > 1, "Invalid process identity");
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("Process liveness unreadable"),
    };
    let state = stat.rsplit_once(") ").and_then(|(_, fields)| fields.split_whitespace().next())
        .ok_or_else(|| anyhow::anyhow!("Invalid process stat"))?;
    Ok(!matches!(state, "Z" | "X"))
}

#[cfg(all(unix, not(target_os = "linux")))]
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
    #[cfg(target_os = "linux")]
    if column == "args" {
        let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).context("Process identity unreadable")?;
        let args = String::from_utf8(bytes)?.split('\0').filter(|arg| !arg.is_empty())
            .collect::<Vec<_>>().join(" ");
        anyhow::ensure!(!args.is_empty(), "Process identity unreadable for pid {pid}");
        return Ok(args);
    }
    let output = std::process::Command::new("ps")
        .env("TZ", "UTC0").env("LC_ALL", "C")
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

#[cfg(all(test, target_os = "linux"))]
#[test]
fn caller_owned_linux_probe_reads_live_proc_identity_and_never_reaps_an_exited_child() {
    let _permit=crate::support::test_subprocess::acquire();
    let mut command=std::process::Command::new("sh");
    command.args(["-c","read line; exit 7","__run-task","t-linux-probe"])
        .stdin(std::process::Stdio::piped());
    let mut child=command.spawn().expect("child");
    assert!(alive(child.id()).expect("proc state"));
    assert!(field(child.id(),"args").expect("proc cmdline").contains("__run-task t-linux-probe"));
    drop(child.stdin.take());
    assert!(crate::support::test_subprocess::wait_dead(child.id() as i32,std::time::Duration::from_secs(5)));
    assert!(!alive(child.id()).expect("exited state"));
    assert_eq!(child.wait().expect("only owner reaps").code(),Some(7));
}
