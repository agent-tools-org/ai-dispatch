// Worktree tests for reuse/prune behavior. Exports test helpers and validates Cargo sync deps.
// Purpose: Guard worktree creation when branches already exist. Deps: tempfile, std.
use super::*;
use crate::test_subprocess;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

fn git(repo_dir: &Path, args: &[&str]) {
    let repo_dir = repo_dir.to_string_lossy().to_string();
    assert!(Command::new("git")
        .args(["-C", repo_dir.as_str()])
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn unique_branch(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

#[test]
fn validate_git_repo_fails_on_nonrepo() {
    assert!(validate_git_repo(Path::new("/tmp")).is_err());
}

#[test]
fn validate_git_repo_succeeds_on_real_repo() {
    assert!(validate_git_repo(Path::new(env!("CARGO_MANIFEST_DIR"))).is_ok());
}

#[test]
fn create_worktree_rejects_invalid_branch_name() {
    let _permit = test_subprocess::acquire();
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);

    let err = create_worktree(repo.path(), "../escape", None).unwrap_err();
    assert!(err.to_string().contains("Invalid branch name"));
}

#[test]
fn create_worktree_with_base_branch_inherits_base_content() {
    let _permit = test_subprocess::acquire();
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("base.txt"), "main\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);

    let base_branch = unique_branch("base");
    git(repo.path(), &["checkout", "-b", base_branch.as_str()]);
    std::fs::write(repo.path().join("inherited.txt"), "from base\n").unwrap();
    git(repo.path(), &["add", "inherited.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    git(repo.path(), &["checkout", "main"]);

    let retry_branch = unique_branch("retry");
    let info = create_worktree(
        repo.path(),
        retry_branch.as_str(),
        Some(base_branch.as_str()),
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(info.path.join("inherited.txt")).unwrap(),
        "from base\n"
    );
    git(
        repo.path(),
        &[
            "worktree",
            "remove",
            "--force",
            &info.path.to_string_lossy(),
        ],
    );
}

#[test]
fn create_worktree_syncs_cargo_lock() {
    let _permit = test_subprocess::acquire();
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    std::fs::write(repo.path().join("Cargo.lock"), "# lock content\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "init"]);

    let branch = unique_branch("feat/cargo-lock-test");
    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();
    assert!(info.path.join("Cargo.lock").exists());
    std::fs::write(repo.path().join("Cargo.lock"), "# updated lock\n").unwrap();
    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();
    assert_eq!(
        std::fs::read_to_string(info.path.join("Cargo.lock")).unwrap(),
        "# updated lock\n"
    );

    git(
        repo.path(),
        &[
            "worktree",
            "remove",
            "--force",
            &info.path.to_string_lossy(),
        ],
    );
}

#[test]
fn create_worktree_reuses_existing_branch_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "init"]);

    let branch = unique_branch("feat/reuse");
    let existing_root = TempDir::new().unwrap();
    let existing_path = existing_root.path().join("worktree");
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            branch.as_str(),
            existing_path.to_str().unwrap(),
        ],
    );

    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();
    // Canonicalize to handle macOS /var → /private/var symlink
    let actual = info.path.canonicalize().unwrap_or(info.path.clone());
    let expected = existing_path.canonicalize().unwrap_or(existing_path.clone());
    assert_eq!(actual, expected);

    git(
        repo.path(),
        &[
            "worktree",
            "remove",
            "--force",
            &existing_path.to_string_lossy(),
        ],
    );
}

#[test]
fn create_worktree_prunes_orphaned_branch_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "init"]);

    let branch = unique_branch("feat/orphan");
    let orphan_root = TempDir::new().unwrap();
    let orphan_path = orphan_root.path().join("worktree");
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            branch.as_str(),
            orphan_path.to_str().unwrap(),
        ],
    );
    std::fs::remove_dir_all(&orphan_path).unwrap();

    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();
    let expected_path = PathBuf::from(format!("/tmp/aid-wt-{branch}"));
    assert_eq!(info.path, expected_path);

    git(
        repo.path(),
        &[
            "worktree",
            "remove",
            "--force",
            &info.path.to_string_lossy(),
        ],
    );
}

#[test]
fn worktree_changed_files_reports_committed_files() {
    let _permit = test_subprocess::acquire();
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("base.txt"), "main").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    git(repo.path(), &["checkout", "-b", "agent-branch"]);

    std::fs::write(repo.path().join("agent.txt"), "one").unwrap();
    git(repo.path(), &["add", "agent.txt"]);
    git(repo.path(), &["commit", "-m", "agent one"]);

    std::fs::write(repo.path().join("agent2.txt"), "two").unwrap();
    git(repo.path(), &["add", "agent2.txt"]);
    git(repo.path(), &["commit", "-m", "agent two"]);

    let files = worktree_changed_files(repo.path()).unwrap();
    assert!(files.contains(&"agent.txt".to_string()));
    assert!(files.contains(&"agent2.txt".to_string()));
}

#[test]
fn create_worktree_rejects_non_aid_branch_on_force_reset_fallback() {
    let _permit = test_subprocess::acquire();
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);

    let branch = unique_branch("legacy");
    git(repo.path(), &["branch", branch.as_str()]);

    let err = create_worktree(repo.path(), branch.as_str(), None).unwrap_err();
    assert!(err.to_string().contains("Refusing to force-reset branch"));
}

#[test]
fn create_worktree_allows_aid_branch_on_force_reset_fallback() {
    let _permit = test_subprocess::acquire();
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);

    let branch = unique_branch("feat/reset");
    git(repo.path(), &["branch", branch.as_str()]);

    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();
    assert!(info.path.exists());
    git(
        repo.path(),
        &["worktree", "remove", "--force", &info.path.to_string_lossy()],
    );
}
