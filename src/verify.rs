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
    let cmd_str = match command {
        Some(c) => c.to_string(),
        None => auto_detect_command(worktree_path),
    };

    if cmd_str == "skip" {
        return Ok(VerifyResult {
            success: true,
            output: "No project file detected, skipping verification".to_string(),
            command: cmd_str,
        });
    }

    let output = Command::new("sh")
        .args(["-c", &cmd_str])
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
        "cargo check 2>&1".to_string()
    } else if path.join("package.json").exists() {
        "npm run build 2>&1".to_string()
    } else {
        "skip".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn verify_pass_case() {
        let dir = TempDir::new().unwrap();
        let result = run_verify(dir.path(), Some("echo ok")).unwrap();
        assert!(result.success);
        assert!(result.output.contains("ok"));
        assert_eq!(result.command, "echo ok");
    }

    #[test]
    fn verify_fail_case() {
        let dir = TempDir::new().unwrap();
        let result = run_verify(dir.path(), Some("sh -c 'echo bad >&2; exit 1'")).unwrap();
        assert!(!result.success);
        assert!(result.output.contains("bad"));
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
