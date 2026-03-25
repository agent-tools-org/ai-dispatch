// Verification tests for command detection and runner behavior.
// Exports: none.
// Deps: crate::verify, tempfile, subprocess helpers.

use super::*;
use crate::store::Store;
use crate::test_subprocess;
use crate::types::{AgentKind, Task, TaskStatus};
use chrono::Local;
use std::fs;
use std::io::Error;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

fn make_task(id: &str, status: TaskStatus, verify_status: VerifyStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None, prompt: "test prompt".to_string(), resolved_prompt: None, status,
        category: None,
        parent_task_id: None, workgroup_id: None, caller_kind: None, caller_session_id: None,
        agent_session_id: None, repo_path: None, worktree_path: None, worktree_branch: None,
        log_path: None, output_path: None, tokens: None, prompt_tokens: None, duration_ms: None,
        model: None, cost_usd: None, exit_code: None,
        created_at: Local::now(),
        completed_at: None, verify: None, verify_status, pending_reason: None, read_only: false, budget: false,
    }
}

#[test]
fn auto_detects_pyproject_projects() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("pyproject.toml"), "[project]\nname='demo'\n").unwrap();
    assert_eq!(auto_detect_command(dir.path()), "python3 -m compileall -q .");
}

#[test]
fn auto_detects_setup_py_projects() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("setup.py"), "from setuptools import setup\n").unwrap();
    assert_eq!(auto_detect_command(dir.path()), "python3 -m compileall -q .");
}

#[test]
fn auto_detects_python_files_without_project_metadata() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("main.py"), "print('ok')\n").unwrap();
    assert_eq!(auto_detect_command(dir.path()), "python3 -m compileall -q .");
}

#[test]
fn rust_detection_takes_priority_over_python_files() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname='demo'\nversion='0.1.0'\n").unwrap();
    fs::write(dir.path().join("main.py"), "print('ok')\n").unwrap();
    assert_eq!(auto_detect_command(dir.path()), "cargo check");
}

#[test]
fn auto_detect_skips_empty_directories() {
    let dir = TempDir::new().unwrap();
    assert_eq!(auto_detect_command(dir.path()), "skip");
}

#[test]
fn verify_pass_case() {
    let _permit = test_subprocess::acquire();
    let dir = TempDir::new().unwrap();
    let result = run_verify(dir.path(), Some("echo ok"), None, None).unwrap();
    assert!(result.success);
    assert!(result.output.contains("ok"));
    assert_eq!(result.command, "echo ok");
}

#[test]
fn verify_fail_case() {
    let _permit = test_subprocess::acquire();
    let dir = TempDir::new().unwrap();
    let result = run_verify(dir.path(), Some("false"), None, None).unwrap();
    assert!(!result.success);
}

#[test]
fn verify_does_not_expand_shell_operators() {
    let _permit = test_subprocess::acquire();
    let dir = TempDir::new().unwrap();
    let result = run_verify(dir.path(), Some("echo ok && false"), None, None).unwrap();
    assert!(result.success);
    assert!(result.output.contains("ok && false"));
}

#[test]
fn verify_no_project_file_skips() {
    let dir = TempDir::new().unwrap();
    let result = run_verify(dir.path(), None, None, None).unwrap();
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

#[test]
fn enforce_verify_status_keeps_done_on_vfail() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-verify-failed", TaskStatus::Done, VerifyStatus::Failed);
    store.insert_task(&task).unwrap();
    enforce_verify_status(&store, &task.id);
    let loaded = store.get_task(task.id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Done, "VFAIL should keep Done status");
    assert_eq!(loaded.verify_status, VerifyStatus::Failed);
}

#[test]
fn enforce_verify_status_keeps_done_passed_verify_as_done() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-verify-passed", TaskStatus::Done, VerifyStatus::Passed);
    store.insert_task(&task).unwrap();
    enforce_verify_status(&store, &task.id);
    let loaded = store.get_task(task.id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Done);
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

    let result = run_verify(dir.path(), script.to_str(), None, None).unwrap();
    assert!(result.success);
    let pid = result.output.trim().parse::<i32>().unwrap();
    thread::sleep(Duration::from_millis(200));
    let status = unsafe { libc::kill(pid, 0) };
    assert_eq!(status, -1);
    assert_eq!(Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
}
