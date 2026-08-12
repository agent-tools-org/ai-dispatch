// Regression tests for task-owned output resolution.
// Covers repo-root leak, persisted --dir, and path containment.
// Deps: show::read_task_output / owned_output_path, Task fixtures.

use crate::cmd::show::{missing_owned_output_absence, owned_output_path, read_task_output};
use crate::paths::AidHomeGuard;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;

fn task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: Some("research".to_string()),
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        observed_model: None,
        attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: true,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

/// FAIL 1: a worktree task must not render a sibling's report sitting at the shared repo root.
#[test]
fn read_task_output_never_renders_repo_root_sibling_report() {
    let root = tempfile::tempdir().unwrap();
    let aid_home = root.path().join("aid-home");
    let _aid_home = AidHomeGuard::set(&aid_home);
    let repo = root.path().join("repo");
    let worktree_b = root.path().join("wt-b");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&worktree_b).unwrap();
    let foreign = "FOREIGN_REPO_ROOT_REPORT: cursor premium holds run_dispatch_resolve";
    std::fs::write(repo.join("report.md"), foreign).unwrap();

    let victim = Task {
        repo_path: Some(repo.display().to_string()),
        worktree_path: Some(worktree_b.display().to_string()),
        output_path: Some("report.md".to_string()),
        ..task("t-victim-repo-root")
    };

    let output = read_task_output(&victim);
    assert!(
        output.is_err(),
        "worktree task must not pick up repo-root report.md: {output:?}"
    );
    if let Ok(leaked) = output {
        assert!(
            !leaked.contains("FOREIGN_REPO_ROOT_REPORT"),
            "task B --output leaked the shared repo-root report"
        );
    }
    assert!(
        missing_owned_output_absence(&victim)
            .unwrap()
            .contains("No task-owned output file"),
        "absence must be explicit"
    );
}

/// FAIL 2: a no-worktree `--dir` task must find its own report under the recorded directory.
#[test]
fn read_task_output_resolves_relative_report_from_recorded_effective_dir() {
    let root = tempfile::tempdir().unwrap();
    let aid_home = root.path().join("aid-home");
    let _aid_home = AidHomeGuard::set(&aid_home);
    let dir = root.path().join("audit-dir");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("report.md"), "owned-by-dir-task\n").unwrap();

    let owner = Task {
        effective_dir: Some(dir.display().to_string()),
        output_path: Some("report.md".to_string()),
        ..task("t-dir-owned")
    };

    assert_eq!(read_task_output(&owner).unwrap(), "owned-by-dir-task\n");
}

/// FAIL 3: `..` and symlinks must not escape the task base.
#[test]
fn owned_output_path_rejects_parent_dir_and_symlink_escape() {
    let root = tempfile::tempdir().unwrap();
    let aid_home = root.path().join("aid-home");
    let _aid_home = AidHomeGuard::set(&aid_home);
    let base = root.path().join("task-a");
    let other = root.path().join("task-b");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("report.md"), "FOREIGN_ESCAPED_REPORT\n").unwrap();

    let via_parent = Task {
        effective_dir: Some(base.display().to_string()),
        output_path: Some("../task-b/report.md".to_string()),
        ..task("t-escape-parent")
    };
    assert_eq!(owned_output_path(&via_parent), None);
    assert!(read_task_output(&via_parent).is_err());

    symlink_file(&other.join("report.md"), &base.join("report.md"));
    let via_symlink = Task {
        effective_dir: Some(base.display().to_string()),
        output_path: Some("report.md".to_string()),
        ..task("t-escape-symlink")
    };
    assert_eq!(owned_output_path(&via_symlink), None);
    assert!(read_task_output(&via_symlink).is_err());
}

fn symlink_file(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(not(unix))]
    panic!("symlink escape test requires unix");
}