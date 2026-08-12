// Tests for merge worktree isolation before group-level side effects.
// Covers group approval and GitButler lane setup guard ordering.
// Deps: merge handlers, Store, temp repos, command shims.

use super::*;
use crate::test_subprocess;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::env;
use std::path::{Path, PathBuf};

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-C", &repo.to_string_lossy()])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

fn init_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@aid.dev"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    std::fs::write(repo.path().join("init.txt"), "init\n").unwrap();
    git(repo.path(), &["add", "init.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

fn grouped_task(id: &str, group: &str, repo: &Path) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "test".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: Some(group.to_string()),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: Some(repo.to_string_lossy().to_string()),
        worktree_path: Some(repo.to_string_lossy().to_string()),
        worktree_branch: Some("main".to_string()),
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn merge_group_with_output_refuses_first_poisoned_task_before_approval() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("hiboss.log");
    write_command(temp.path(), "hiboss", &format!("printf called > '{}'\n", log.display()));
    crate::cmd::merge::set_test_hiboss_command(Some(temp.path().join("hiboss").to_string_lossy().to_string()));
    let store = Store::open_memory().unwrap();
    let group = "wg-poisoned-merge";
    let mut task = grouped_task("t-poisoned-first", group, repo.path());
    task.created_at = Local::now();
    store.insert_task(&task).unwrap();

    let err = merge_group_with_output(&store, group, true, false, false, None, false)
        .unwrap_err();

    assert!(err.to_string().contains("recorded worktree path"));
    assert!(!log.exists(), "approval ran before poisoned worktree validation");
    crate::cmd::merge::set_test_hiboss_command(None);
}

#[test]
fn merge_group_lanes_refuses_first_poisoned_task_before_gitbutler_setup() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    std::fs::create_dir(repo.path().join(".aid")).unwrap();
    std::fs::write(
        repo.path().join(".aid/project.toml"),
        "[project]\nid = \"demo\"\ngitbutler = \"auto\"\n",
    )
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let setup_log = temp.path().join("setup.log");
    crate::gitbutler::set_test_but_available(Some(true));
    crate::gitbutler::set_test_project_present(Some(true));
    write_command(temp.path(), "but", &format!("pwd > '{}'\n", setup_log.display()));
    crate::gitbutler::set_test_but_command(Some(temp.path().join("but").to_string_lossy().to_string()));
    let store = Store::open_memory().unwrap();
    let group = "wg-poisoned-lanes";
    let mut task = grouped_task("t-poisoned-lanes-first", group, repo.path());
    task.created_at = Local::now();
    store.insert_task(&task).unwrap();

    let err = merge_lanes::merge_group_lanes(&store, group, false).unwrap_err();

    assert!(err.to_string().contains("recorded worktree path"));
    assert!(!setup_log.exists(), "GitButler setup ran before worktree validation");
    
    crate::gitbutler::set_test_but_available(None);
    crate::gitbutler::set_test_project_present(None);
    crate::gitbutler::set_test_but_command(None);
}

#[test]
fn merge_group_lanes_refuses_failed_verification_without_force() {
    let store = Store::open_memory().unwrap();
    let group = "wg-lanes-vfail";
    let mut task = grouped_task("t-lanes-vfail", group, Path::new("."));
    task.worktree_path = None;
    task.worktree_branch = None;
    task.repo_path = None;
    task.verify_status = VerifyStatus::Failed;
    store.insert_task(&task).unwrap();
    
    // Test that validation fails on vfail without force.
    // Use thread_local to ensure we don't bail out prematurely on AID_GITBUTLER=0 check if we reach it.
    crate::gitbutler::set_test_but_available(Some(true));

    let err = merge_lanes::merge_group_lanes(&store, group, false).unwrap_err();

    assert!(err.to_string().contains("verification failed"));
    crate::gitbutler::set_test_but_available(None);
}

#[test]
fn merge_group_lanes_skips_an_already_merged_task() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    git(repo.path(), &["switch", "-c", "lane"]);
    std::fs::write(repo.path().join("lane.txt"), "lane\n").unwrap();
    git(repo.path(), &["add", "lane.txt"]);
    git(repo.path(), &["commit", "-m", "lane"]);
    git(repo.path(), &["switch", "main"]);
    std::fs::create_dir(repo.path().join(".aid")).unwrap();
    std::fs::write(
        repo.path().join(".aid/project.toml"),
        "[project]\nid = \"demo\"\ngitbutler = \"auto\"\n",
    )
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let apply_log = temp.path().join("apply.log");
    
    // Instead of PathGuard, EnvGuard, CurrentDirGuard which mutate process-wide state,
    // we use thread-local overrides injected into gitbutler logic.
    crate::gitbutler::set_test_but_available(Some(true));
    crate::gitbutler::set_test_project_present(Some(true));
    crate::gitbutler::set_test_but_command(Some(temp.path().join("but").to_string_lossy().to_string()));

    write_command(
        temp.path(),
        "but",
        &format!("case \"$1\" in apply) printf applied > '{}';; esac\n", apply_log.display()),
    );
    let store = Store::open_memory().unwrap();
    let group = "wg-lanes-already-merged";
    let mut task = grouped_task("t-lanes-already-merged", group, repo.path());
    task.status = TaskStatus::Merged;
    task.worktree_path = None;
    task.worktree_branch = Some("lane".to_string());
    store.insert_task(&task).unwrap();

    merge_lanes::merge_group_lanes(&store, group, false).unwrap();

    assert!(!apply_log.exists());
    assert_eq!(store.get_task(task.id.as_str()).unwrap().unwrap().status, TaskStatus::Merged);

    crate::gitbutler::set_test_but_available(None);
    crate::gitbutler::set_test_project_present(None);
    crate::gitbutler::set_test_but_command(None);
}

fn write_command(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
    }
    std::fs::set_permissions(path, perms).unwrap();
}
