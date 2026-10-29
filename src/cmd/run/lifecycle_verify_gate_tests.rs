// Lifecycle verify gate regressions for completed worktree tasks.
// Covers normal worktree completion, dirty-rescue completion, and the single
// settled-state backup. Deps: post_run_lifecycle, Store, git CLI, tempfile.

use super::{
    RunArgs,
    run_lifecycle::{LifecycleMode, post_run_lifecycle},
    run_prompt::PromptBundle,
};
use crate::{
    store::Store,
    support::test_subprocess,
    types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus},
};

use std::{path::{Path, PathBuf}, process::Command, sync::Arc};

#[path = "lifecycle_read_only_backup_tests.rs"]
mod read_only_backup_tests;

use crate::support::test_env::git;

fn init_repo() -> tempfile::TempDir {
    crate::support::test_repo::repo_with_file("base.txt", "base\n", "base")
}

fn create_worktree(repo: &Path, branch: &str) -> PathBuf {
    let info = crate::workspace::worktree::create_worktree(repo, branch, None).unwrap();
    git(&info.path, &["config", "user.email", "test@example.com"]);
    git(&info.path, &["config", "user.name", "Test User"]);
    info.path
}

fn task(id: &str, repo: &Path, wt: &Path, branch: &str) -> Task {
    Task {
        repo_path: Some(repo.display().to_string()),
        worktree_path: Some(wt.display().to_string()),
        worktree_branch: Some(branch.to_string()),
        duration_ms: Some(1_000),
        exit_code: Some(0),
        verify: Some("false".to_string()),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Done)
    }
}

fn prompt_bundle() -> PromptBundle {
    PromptBundle {
        effective_prompt: "prompt".to_string(),
        ..Default::default()
    }
}

async fn run_lifecycle(store: &Arc<Store>, task_id: &TaskId, args: &RunArgs) {
    post_run_lifecycle(
        LifecycleMode::Background,
        store,
        task_id,
        args,
        AgentKind::Codex,
        "codex",
        args.dir.as_ref(),
        args.repo.as_ref(),
        args.dir.as_ref(),
        None,
        &[],
        &prompt_bundle(),
        TaskStatus::Done,
        None,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn failed_verify_fails_completed_worktree_task() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = "fix/verify-gate-clean";
    let wt = create_worktree(repo.path(), branch);
    // Real change optional now that empty-diff no longer skips verify; keep one
    // so this case is clearly "agent produced work that still failed verify".
    std::fs::write(wt.join("change.txt"), "changed\n").unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-vgate-worktree".to_string());
    store
        .insert_task(&task(task_id.as_str(), repo.path(), &wt, branch))
        .unwrap();
    let args = RunArgs {
        repo: Some(repo.path().display().to_string()),
        dir: Some(wt.display().to_string()),
        verify: Some("false".to_string()),
        ..Default::default()
    };

    run_lifecycle(&store, &task_id, &args).await;

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.verify_status, VerifyStatus::Failed);
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.exit_code, Some(1));
}

#[tokio::test]
async fn failed_verify_fails_after_dirty_rescue_path() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = "fix/verify-gate-rescue";
    let wt = create_worktree(repo.path(), branch);
    std::fs::write(wt.join("rescued.txt"), "rescued\n").unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-vgate-rescue".to_string());
    store
        .insert_task(&task(task_id.as_str(), repo.path(), &wt, branch))
        .unwrap();
    let args = RunArgs {
        repo: Some(repo.path().display().to_string()),
        dir: Some(wt.display().to_string()),
        verify: Some("false".to_string()),
        ..Default::default()
    };

    run_lifecycle(&store, &task_id, &args).await;

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    let events = store.get_events(task_id.as_str()).unwrap();
    assert_eq!(task.verify_status, VerifyStatus::Failed);
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.exit_code, Some(1));
    assert!(events.iter().any(|event| event.detail.contains("Rescued 1 file")));
}

/// A fake `gws` that logs argv and copies any `--upload`ed file into `capture`.
fn capturing_gws(home: &Path, capture: &Path) -> PathBuf {
    let log = home.join("gws.log");
    let binary = home.join("gws");
    let script = format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\nfor a in \"$@\"; do last=$a; done\n\
         [ -f \"$last\" ] && cp \"$last\" '{}'/\necho '{{\"id\":\"fake123\"}}'\n",
        log.display(),
        capture.display()
    );
    crate::support::test_env::write_executable(&binary, &script);
    std::fs::write(
        crate::support::paths::config_path(),
        format!("[backup.gdrive]\nbinary = '{}'\n", binary.display()),
    )
    .unwrap();
    log
}

#[tokio::test]
async fn backup_runs_once_after_the_verify_gate_settles_the_task() {
    let _permit = test_subprocess::acquire();
    let home = crate::support::test_env::TestHome::new();
    let capture = home.path().join("capture");
    std::fs::create_dir_all(&capture).unwrap();
    let log = capturing_gws(home.path(), &capture);
    let repo = init_repo();
    let branch = "fix/verify-gate-backup";
    let wt = create_worktree(repo.path(), branch);
    std::fs::write(wt.join("change.txt"), "changed\n").unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-vgate-backup".to_string());
    store.insert_task(&task(task_id.as_str(), repo.path(), &wt, branch)).unwrap();
    let args = RunArgs {
        repo: Some(repo.path().display().to_string()),
        dir: Some(wt.display().to_string()),
        verify: Some("false".to_string()),
        backup: Some("gdrive:audits".to_string()),
        ..Default::default()
    };
    store.update_task_dispatch_args(task_id.as_str(), &args.dispatch_args_json().unwrap()).unwrap();

    run_lifecycle(&store, &task_id, &args).await;

    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Failed);
    let calls = std::fs::read_to_string(&log).unwrap();
    assert_eq!(calls.lines().filter(|l| l.contains("--upload")).count(), 1, "{calls}");
    let events = store.get_events(task_id.as_str()).unwrap();
    let attempts: Vec<_> = events
        .iter()
        .filter(|e| e.metadata.as_ref().is_some_and(|m| m.get("backup").is_some()))
        .collect();
    assert_eq!(attempts.len(), 1, "{events:?}");
    assert_eq!(attempts[0].metadata.as_ref().unwrap()["backup"], "uploaded");
    assert!(store.backup_url(task_id.as_str()).unwrap().is_some());
    let bundle = std::fs::read_dir(&capture).unwrap().next().unwrap().unwrap().path();
    let export = Command::new("tar").args(["-xzOf"]).arg(&bundle).arg("./export.md").output().unwrap();
    let export = String::from_utf8_lossy(&export.stdout);
    assert!(
        export.contains(&format!("Status: {}", TaskStatus::Failed.as_str())),
        "bundle must carry the settled status:\n{export}"
    );
}
