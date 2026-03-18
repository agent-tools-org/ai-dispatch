// Verification runner: checks agent output by running a command in the worktree.
// Supports auto-detect (Cargo.toml -> cargo check, package.json -> npm run build).

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::store::Store;
use crate::types::{TaskId, VerifyStatus};

#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub success: bool,
    pub output: String,
    pub command: String,
}

/// Run a verification command in the given worktree directory.
/// If `command` is None, auto-detect based on project files.
pub fn run_verify(worktree_path: &Path, command: Option<&str>) -> Result<VerifyResult> {
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
    let output = cmd
        .current_dir(worktree_path)
        .output()
        .with_context(|| format!("Failed to run verify command: {cmd_str}"))?;

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    Ok(VerifyResult {
        success: output.status.success(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_subprocess;
    use tempfile::TempDir;

    #[test]
    fn verify_pass_case() {
        let _permit = test_subprocess::acquire();
        let dir = TempDir::new().unwrap();
        let result = run_verify(dir.path(), Some("echo ok")).unwrap();
        assert!(result.success);
        assert!(result.output.contains("ok"));
        assert_eq!(result.command, "echo ok");
    }

    #[test]
    fn verify_fail_case() {
        let _permit = test_subprocess::acquire();
        let dir = TempDir::new().unwrap();
        let result = run_verify(dir.path(), Some("false")).unwrap();
        assert!(!result.success);
    }

    #[test]
    fn verify_does_not_expand_shell_operators() {
        let _permit = test_subprocess::acquire();
        let dir = TempDir::new().unwrap();
        let result = run_verify(dir.path(), Some("echo ok && false")).unwrap();
        assert!(result.success);
        assert!(result.output.contains("ok && false"));
    }

    #[test]
    fn verify_no_project_file_skips() {
        let dir = TempDir::new().unwrap();
        let result = run_verify(dir.path(), None).unwrap();
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
}
