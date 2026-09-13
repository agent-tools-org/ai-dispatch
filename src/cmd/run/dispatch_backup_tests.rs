// A dispatch that fails during setup, before its args are persisted, makes no
// backup attempt even when the project configures one and `--no-backup` was
// passed on the command line. Deps: prepare_dispatch_with, Store, git CLI.

use super::*;
use std::process::Command;
use std::sync::Arc;

fn git(repo_dir: &Path, args: &[&str]) {
    let status = Command::new("git").arg("-C").arg(repo_dir).args(args).status().unwrap();
    assert!(status.success(), "git {args:?}");
}

fn repo_with_backup_project() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::create_dir_all(repo.path().join(".aid")).unwrap();
    std::fs::write(
        repo.path().join(".aid/project.toml"),
        "[project]\nid = 'proj'\n[backup]\ntarget = 'nope'\non = ['fail', 'complete']\n",
    )
    .unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

#[test]
fn setup_failure_with_no_backup_makes_zero_backup_attempts() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let _permit = crate::test_subprocess::acquire();
    let repo = repo_with_backup_project();
    let store = Arc::new(Store::open_memory().unwrap());
    git(repo.path(), &["checkout", "-b", "chore/root-branch"]);
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        existing_task_id: Some(TaskId("t-nb-setup".to_string())),
        repo_root: Some(repo.path().display().to_string()),
        dir: Some(repo.path().display().to_string()),
        worktree: Some("chore/root-branch".to_string()),
        no_backup: true,
        ..Default::default()
    };

    let err = match prepare_dispatch_with(&store, &mut args, |_| true) {
        Ok(_) => panic!("repo-root worktree must be rejected"),
        Err(err) => err.to_string(),
    };

    assert!(err.contains("main working tree"), "{err}");
    let task = store.get_task("t-nb-setup").unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Failed);
    assert!(store.get_task_dispatch_args("t-nb-setup").unwrap().is_none(), "args never persisted");
    let events = store.get_events("t-nb-setup").unwrap();
    assert!(!events.iter().any(|e| e.detail.contains("Backup")), "{events:?}");
    assert!(!crate::backup::already_attempted(&store, "t-nb-setup"));
}
