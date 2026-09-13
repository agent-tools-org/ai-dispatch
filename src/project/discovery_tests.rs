// Regression tests for project config discovery in real Git worktrees.
// Exports: none; loaded by project.rs under #[cfg(test)].
// Deps: project discovery and identity, tempfile, std filesystem and Git CLI.

use super::{detect_project_in, project_path_in_repo, resolve_project_id};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn init_repo() -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("main");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(
        &repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ],
    );
    (temp, repo)
}

fn write_main_config(repo: &Path) {
    fs::create_dir(repo.join(".aid")).unwrap();
    fs::write(
        project_path_in_repo(repo),
        "[project]\nid = \"main-project\"\nrules = [\"main rule\"]\n",
    )
    .unwrap();
    let mut exclude = fs::OpenOptions::new()
        .append(true)
        .open(repo.join(".git/info/exclude"))
        .unwrap();
    writeln!(exclude, "\n.aid/").unwrap();
}

fn detached_worktree(repo: &Path, temp: &TempDir) -> PathBuf {
    let detached = temp.path().join("detached");
    let sha = git(repo, &["rev-parse", "HEAD"]);
    git(
        repo,
        &[
            "worktree",
            "add",
            "--detach",
            &detached.to_string_lossy(),
            &sha,
        ],
    );
    detached
}

#[test]
fn detached_worktree_inherits_main_config() {
    let (temp, repo) = init_repo();
    write_main_config(&repo);
    let detached = detached_worktree(&repo, &temp);
    assert!(!project_path_in_repo(&detached).exists());

    let config = detect_project_in(&detached).unwrap();
    assert_eq!(config.id, "main-project");
    assert_eq!(config.rules, ["main rule"]);
}

#[test]
fn detached_worktree_local_config_wins_but_identity_stays_main_first() {
    let (temp, repo) = init_repo();
    write_main_config(&repo);
    let detached = detached_worktree(&repo, &temp);
    fs::create_dir(detached.join(".aid")).unwrap();
    fs::write(
        project_path_in_repo(&detached),
        "[project]\nid = \"local-project\"\nrules = [\"local rule\"]\n",
    )
    .unwrap();

    let config = detect_project_in(&detached).unwrap();
    assert_eq!(config.id, "local-project");
    assert_eq!(config.rules, ["local rule"]);
    assert_eq!(
        resolve_project_id(&detached).as_deref(),
        Some("main-project")
    );
}

#[test]
fn plain_repo_without_config_returns_none() {
    let (_temp, repo) = init_repo();
    assert!(detect_project_in(&repo).is_none());
}

#[test]
fn detached_worktree_without_any_config_returns_none() {
    let (temp, repo) = init_repo();
    let detached = detached_worktree(&repo, &temp);
    assert!(detect_project_in(&detached).is_none());
}
