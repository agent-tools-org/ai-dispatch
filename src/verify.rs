// Verification runner: checks agent output by running a command in the worktree.
// Supports auto-detect (Cargo.toml -> cargo check, package.json -> npm run build).

use anyhow::{Context, Result};
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use crate::process_guard::ProcessGuard;
use crate::store::Store;
use crate::types::{TaskId, VerifyStatus};

static VERIFY_LOCK: Mutex<()> = Mutex::new(());
const VERIFY_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub success: bool,
    pub output: String,
    pub command: String,
}

/// Run a verification command in the given worktree directory.
/// If `command` is None, auto-detect based on project files.
pub fn run_verify(
    worktree_path: &Path,
    command: Option<&str>,
    cargo_target_dir: Option<&str>,
) -> Result<VerifyResult> {
    let Some((cmd_str, mut cmd)) = build_verify_command(worktree_path, command)? else {
        return Ok(VerifyResult {
            success: true,
            output: "No project file detected, skipping verification".to_string(),
            command: "skip".to_string(),
        });
    };

    // Verify commands are user-authored, but we still execute them as argv to avoid
    // handing arbitrary strings to a shell. This preserves simple commands like
    // `cargo test` while refusing shell metacharacter expansion by default.
    if let Some(target_dir) = cargo_target_dir {
        cmd.env("CARGO_TARGET_DIR", target_dir);
    }

    let _lock = VERIFY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    cmd.current_dir(worktree_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut guard = ProcessGuard::spawn(&mut cmd)
        .with_context(|| format!("Failed to run verify command: {cmd_str}"))?;
    let reader = spawn_output_reader(guard.child_mut())?;
    let status = guard.wait_with_timeout(VERIFY_TIMEOUT)?;
    let timed_out = status.is_none();
    let combined = match reader.join() {
        Ok(result) => result?,
        Err(_) => return Err(anyhow::anyhow!("verify output reader thread panicked")),
    };
    let combined = if timed_out {
        format!("{combined}\nVerification timed out after {} seconds", VERIFY_TIMEOUT.as_secs())
    } else {
        combined
    };

    Ok(VerifyResult {
        success: status.is_some_and(|status| status.success()),
        output: combined,
        command: cmd_str,
    })
}
/// Format a concise pass/fail report from verification result.
pub fn format_verify_report(result: &VerifyResult) -> String {
    let status = if result.success { "PASS" } else { "FAIL" };
    let mut report = format!("Verify {status} ({}):", result.command);
    if !result.success {
        // Show last 20 lines of output on failure
        let lines: Vec<&str> = result.output.lines().collect();
        let start = lines.len().saturating_sub(20);
        if start > 0 {
            report.push_str(&format!("\n  ... ({start} lines omitted)"));
        }
        for line in &lines[start..] {
            report.push_str(&format!("\n  {line}"));
        }
    }
    report
}

/// Update the task's verify_status based on the latest verify result.
pub fn record_verify_status(store: &Store, task_id: &TaskId, result: &VerifyResult) {
    if result.command == "skip" {
        return;
    }
    let status = if result.success {
        VerifyStatus::Passed
    } else {
        VerifyStatus::Failed
    };
    let _ = store.update_verify_status(task_id.as_str(), status);
}
fn auto_detect_command(path: &Path) -> String {
    if path.join("Cargo.toml").exists() {
        "cargo check".to_string()
    } else if path.join("package.json").exists() {
        "npm run build".to_string()
    } else {
        "skip".to_string()
    }
}

fn build_verify_command(
    worktree_path: &Path,
    command: Option<&str>,
) -> Result<Option<(String, Command)>> {
    let cmd_str = match command {
        Some(c) => c.trim().to_string(),
        None => auto_detect_command(worktree_path),
    };
    if cmd_str == "skip" {
        return Ok(None);
    }
    split_command(&cmd_str).map(Some)
}
fn split_command(command: &str) -> Result<(String, Command)> {
    let mut parts = command.split_whitespace();
    let program = parts.next().context("verify command is empty")?;
    let args: Vec<&str> = parts.collect();
    let mut cmd = Command::new(program);
    cmd.args(&args);
    Ok((command.to_string(), cmd))
}
fn spawn_output_reader(child: &mut Child) -> Result<thread::JoinHandle<Result<String>>> {
    let stdout = child.stdout.take().context("verify stdout pipe missing")?;
    let stderr = child.stderr.take().context("verify stderr pipe missing")?;
    Ok(thread::spawn(move || {
        let stdout_handle = thread::spawn(move || read_pipe(stdout));
        let stderr_handle = thread::spawn(move || read_pipe(stderr));
        let stdout = stdout_handle
            .join()
            .map_err(|_| anyhow::anyhow!("verify stdout reader thread panicked"))??;
        let stderr = stderr_handle
            .join()
            .map_err(|_| anyhow::anyhow!("verify stderr reader thread panicked"))??;
        Ok(format!("{stdout}{stderr}"))
    }))
}
fn read_pipe<R: Read + Send + 'static>(mut reader: R) -> Result<String> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_subprocess;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;
    #[test]
    fn verify_pass_case() {
        let _permit = test_subprocess::acquire();
        let dir = TempDir::new().unwrap();
        let result = run_verify(dir.path(), Some("echo ok"), None).unwrap();
        assert!(result.success);
        assert!(result.output.contains("ok"));
        assert_eq!(result.command, "echo ok");
    }
    #[test]
    fn verify_fail_case() {
        let _permit = test_subprocess::acquire();
        let dir = TempDir::new().unwrap();
        let result = run_verify(dir.path(), Some("false"), None).unwrap();
        assert!(!result.success);
    }
    #[test]
    fn verify_does_not_expand_shell_operators() {
        let _permit = test_subprocess::acquire();
        let dir = TempDir::new().unwrap();
        let result = run_verify(dir.path(), Some("echo ok && false"), None).unwrap();
        assert!(result.success);
        assert!(result.output.contains("ok && false"));
    }
    #[test]
    fn verify_no_project_file_skips() {
        let dir = TempDir::new().unwrap();
        let result = run_verify(dir.path(), None, None).unwrap();
        assert!(result.success);
        assert!(result.output.contains("skipping"));
    }
    #[test]
    fn format_report_pass() {
        let result = VerifyResult {
            success: true,
            output: "all good".to_string(),
            command: "cargo check".to_string(),
        };
        let report = format_verify_report(&result);
        assert!(report.starts_with("Verify PASS"));
    }
    #[test]
    fn format_report_fail_shows_output() {
        let result = VerifyResult {
            success: false,
            output: "error[E0308]: mismatched types".to_string(),
            command: "cargo check".to_string(),
        };
        let report = format_verify_report(&result);
        assert!(report.contains("FAIL"));
        assert!(report.contains("mismatched types"));
    }
    #[cfg(unix)]
    #[test]
    fn verify_kills_background_grandchildren() {
        let _permit = test_subprocess::acquire();
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("verify-cleanup.sh");
        fs::write(
            &script,
            "#!/bin/sh\nsleep 30 &\nchild_pid=$!\necho \"$child_pid\"\nexit 0\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();

        let result = run_verify(dir.path(), script.to_str(), None).unwrap();
        assert!(result.success);
        let pid = result.output.trim().parse::<i32>().unwrap();
        thread::sleep(Duration::from_millis(200));
        let status = unsafe { libc::kill(pid, 0) };
        assert_eq!(status, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }
}
