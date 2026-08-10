// Verification runner: checks agent output by running a command in the worktree.
// Supports auto-detect for Rust, Node, and Python projects.

use anyhow::{Context, Result};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use crate::process_guard::ProcessGuard;
use crate::store::Store;
use crate::types::{TaskId, TaskStatus, VerifyStatus};

static VERIFY_LOCK: Mutex<()> = Mutex::new(());
pub(crate) const VERIFY_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub success: bool,
    /// True when the verify command was killed by the wall-clock cap.
    /// Distinct from `success == false`: a timeout did not finish, so it is not
    /// evidence that the change under test is broken.
    pub timed_out: bool,
    pub output: String,
    pub command: String,
}

/// Run a verification command in the given worktree directory.
/// If `command` is None, auto-detect based on project files.
pub fn run_verify(
    worktree_path: &Path,
    command: Option<&str>,
    cargo_target_dir: Option<&str>,
    container_name: Option<&str>,
) -> Result<VerifyResult> {
    run_verify_with_timeout(
        worktree_path,
        command,
        cargo_target_dir,
        container_name,
        VERIFY_TIMEOUT,
    )
}
pub(crate) fn run_verify_with_timeout(
    worktree_path: &Path,
    command: Option<&str>,
    cargo_target_dir: Option<&str>,
    container_name: Option<&str>,
    timeout: Duration,
) -> Result<VerifyResult> {
    if command.is_some_and(|command| command.trim() == "skip") {
        return Ok(VerifyResult {
            success: false,
            timed_out: false,
            output: "Configured verify command was 'skip' and did not run".to_string(),
            command: "skip".to_string(),
        });
    }
    let Some((cmd_str, mut cmd)) =
        build_verify_command(worktree_path, command, container_name, cargo_target_dir)?
    else {
        return Ok(VerifyResult {
            success: true,
            timed_out: false,
            output: "No project file detected, skipping verification".to_string(),
            command: "skip".to_string(),
        });
    };

    // Verify commands are user-authored, but we still execute them as argv to avoid
    // handing arbitrary strings to a shell. This preserves simple commands like
    // `cargo test` while refusing shell metacharacter expansion by default.
    if container_name.is_none() {
        crate::agent::apply_cargo_target_env(&mut cmd, cargo_target_dir);
    }

    let _lock = VERIFY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    cmd.current_dir(worktree_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut guard = ProcessGuard::spawn(&mut cmd)
        .with_context(|| format!("Failed to run verify command: {cmd_str}"))?;
    let reader = spawn_output_reader(guard.child_mut())?;
    let status = guard.wait_with_timeout(timeout)?;
    let timed_out = status.is_none();
    let combined = match reader.join() {
        Ok(result) => result?,
        Err(_) => return Err(anyhow::anyhow!("verify output reader thread panicked")),
    };
    let combined = if timed_out {
        format!(
            "{combined}\nVerification timed out after {} seconds",
            timeout.as_secs().max(1)
        )
    } else {
        combined
    };
    let success = status.as_ref().is_some_and(|status| status.success());
    Ok(VerifyResult {
        success,
        timed_out,
        output: combined,
        command: cmd_str,
    })
}

/// Format a concise pass/fail report from verification result.
pub fn format_verify_report(result: &VerifyResult) -> String {
    let status = if result.timed_out {
        "TIMEOUT"
    } else if result.success {
        "PASS"
    } else {
        "FAIL"
    };
    let mut report = format!("Verify {status} ({}):", result.command);
    if !result.success || result.timed_out {
        // Show last 20 lines of output on failure / timeout
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
    // `auto` can resolve to `skip`, and the caller cannot know that in advance.
    // Record it, so a task that asked for verification and then legitimately
    // needed none reads as Skipped rather than sitting on the Pending marker.
    if result.command == "skip" && result.success {
        let _ = store.update_verify_status(task_id.as_str(), VerifyStatus::Skipped);
        return;
    }
    let status = if result.timed_out {
        VerifyStatus::TimedOut
    } else if result.success {
        VerifyStatus::Passed
    } else {
        VerifyStatus::Failed
    };
    let _ = store.update_verify_status(task_id.as_str(), status);
}
/// Fail a completed task when verification failed.
pub fn enforce_verify_status(store: &Store, task_id: &TaskId) {
    let Some(task) = store.get_task(task_id.as_str()).ok().flatten() else { return };
    if task.status == TaskStatus::Done && task.verify_status == VerifyStatus::Failed {
        match crate::task_lifecycle::fail_completed_verify_gate(store, task_id) {
            Ok(true) => eprintln!("[aid] Task {task_id} failed because verification failed"),
            Ok(false) => {}
            Err(err) => aid_error!("[aid] Failed to enforce verify result for {task_id}: {err}"),
        }
    }
}
fn auto_detect_command(path: &Path) -> String {
    if path.join("Cargo.toml").exists() {
        "cargo check".to_string()
    } else if path.join("package.json").exists() {
        "npm run build".to_string()
    } else if path.join("pyproject.toml").exists()
        || path.join("setup.py").exists()
        || path.join("setup.cfg").exists()
        || has_python_files(path) {
        "python3 -m compileall -q .".to_string()
    } else {
        "skip".to_string()
    }
}

fn has_python_files(path: &Path) -> bool {
    std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let entry_path = entry.path();
            is_python_file(&entry_path)
                || entry.file_type().ok().is_some_and(|file_type| {
                    file_type.is_dir()
                        && std::fs::read_dir(entry_path)
                            .ok()
                            .into_iter()
                            .flatten()
                            .flatten()
                            .any(|nested| is_python_file(&nested.path()))
                })
        })
}

fn is_python_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "py")
}

fn build_verify_command(
    worktree_path: &Path,
    command: Option<&str>,
    container_name: Option<&str>,
    cargo_target_dir: Option<&str>,
) -> Result<Option<(String, Command)>> {
    let cmd_str = match command {
        Some(c) => c.trim().to_string(),
        None => auto_detect_command(worktree_path),
    };
    if cmd_str == "skip" {
        return Ok(None);
    }
    let (_, cmd) = split_command(&cmd_str)?;
    let cmd = if let Some(container_name) = container_name {
        crate::container::verify_in_container(
            container_name,
            worktree_path,
            &cmd_str,
            cargo_target_dir,
        )
    } else {
        cmd
    };
    Ok(Some((cmd_str, cmd)))
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
#[path = "verify_tests.rs"]
mod tests;
