// Tests for dry-run dispatch behavior in `cmd::run`.
// Exports: none.
// Deps: run entrypoint, Store, paths, git subprocess fixtures.

use super::*;
use crate::store::Store;

use std::sync::Arc;
use tempfile::TempDir;

use crate::support::test_env::git;

#[tokio::test]
async fn dry_run_with_explicit_repo_rejects_missing_effective_dir() {
    let _aid_home = crate::support::test_env::TestHome::with_dirs();
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "aid@example.com"]);
    git(repo.path(), &["config", "user.name", "Aid Tester"]);
    std::fs::write(repo.path().join("file.txt"), "initial").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-m", "initial"]);
    let store = Arc::new(Store::open_memory().unwrap());

    let err = run(store, RunArgs {
        agent_name: "codex".to_string(),
        prompt: "repro".to_string(),
        repo: Some(repo.path().to_string_lossy().to_string()),
        dir: Some("/tmp/does-not-exist/".to_string()),
        worktree: Some("chore/repro-eager".to_string()),
        dry_run: true,
        skills: vec![NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    })
    .await
    .unwrap_err();

    assert!(err.to_string().contains("batch file / task dir missing in worktree"));
}
