// CLI regressions for shared-worktree collection and unrelated artifact custody.
// Exercises acceptance, GC, Git failures, and persisted durability certificates.
// Deps: compiled aid binary, Git, rusqlite, tempfile, and the common CLI harness.

mod common;

use rusqlite::{Connection, params};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn success(output: Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git(repo: &Path, args: &[&str]) -> String {
    success(Command::new("git")
        .args(["-c", "protocol.file.allow=always", "-C"])
        .arg(repo).args(args).output().unwrap())
}

fn init(repo: &Path) {
    fs::create_dir_all(repo).unwrap();
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test User"]);
    fs::write(repo.join("file.txt"), "base").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "base"]);
}

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    worktree: PathBuf,
    db: Connection,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new().prefix("aid-wt-gc-").tempdir_in("/tmp").unwrap();
        let repo = root.path().join("repo");
        init(&repo);
        success(common::aid_cmd_in(root.path()).args(["board", "--json"]).output().unwrap());
        let db = Connection::open(root.path().join("aid.db")).unwrap();
        let worktree = root.path().join("shared");
        git(&repo, &["worktree", "add", worktree.to_str().unwrap(), "-b", "shared"]);
        Self { root, repo, worktree, db }
    }

    fn accept(&self, task: &str, worktree: &Path) {
        self.db.execute(
            "INSERT INTO tasks (id, agent, prompt, status, repo_path, worktree_path, created_at)
             VALUES (?1, 'codex', 'task', 'done', ?2, ?3, ?4)",
            params![task, self.repo.to_str().unwrap(), worktree.to_str().unwrap(),
                chrono::Local::now().to_rfc3339()],
        ).unwrap();
        success(common::aid_cmd_in(self.root.path()).args(["accept", task]).output().unwrap());
    }

    fn gc(&self, task: &str) -> Command {
        let mut command = common::aid_cmd_in(self.root.path());
        command.args(["gc", "--task", task]);
        command
    }

    fn certificates(&self, task: &str) -> i64 {
        self.db.query_row("SELECT COUNT(*) FROM artifact_durability WHERE task_id = ?1",
            [task], |row| row.get(0)).unwrap()
    }
}

#[test]
fn original_and_retry_collect_shared_worktree_with_durability_for_both() {
    let fixture = Fixture::new();
    fixture.accept("t-original", &fixture.worktree);
    fixture.accept("t-retry", &fixture.worktree);
    success(fixture.gc("t-original").output().unwrap());
    assert!(!fixture.worktree.exists());
    assert!(!git(&fixture.repo, &["worktree", "list", "--porcelain"])
        .contains(fixture.worktree.to_str().unwrap()));
    let stdout = success(fixture.gc("t-retry").output().unwrap());
    assert!(stdout.contains("Worktree was already collected"));
    assert_eq!(fixture.certificates("t-original"), 1);
    assert_eq!(fixture.certificates("t-retry"), 1);
}

fn private_submodule(fixture: &Fixture) -> (PathBuf, PathBuf, String) {
    let child = fixture.root.path().join("child");
    init(&child);
    git(&fixture.repo, &["submodule", "add", child.to_str().unwrap(), "lib"]);
    git(&fixture.repo, &["commit", "-am", "submodule"]);
    let other = fixture.root.path().join("other");
    git(&fixture.repo, &["worktree", "add", other.to_str().unwrap(), "-b", "other"]);
    git(&other, &["submodule", "update", "--init"]);
    let sub = other.join("lib");
    git(&sub, &["config", "user.email", "test@example.com"]);
    git(&sub, &["config", "user.name", "Test User"]);
    fs::write(sub.join("file.txt"), "private commit").unwrap();
    git(&sub, &["commit", "-am", "private"]);
    let commit = git(&sub, &["rev-parse", "HEAD"]);
    let private_dir = PathBuf::from(git(&sub, &["rev-parse", "--absolute-git-dir"]));
    git(&other, &["commit", "-am", "advance gitlink"]);
    (other, private_dir, commit)
}

#[test]
fn already_collected_gc_preserves_other_missing_worktree_private_objects() {
    let fixture = Fixture::new();
    fixture.accept("t-original", &fixture.worktree);
    fixture.accept("t-retry", &fixture.worktree);
    success(fixture.gc("t-original").output().unwrap());
    let (other, private_dir, commit) = private_submodule(&fixture);
    fixture.accept("t-other", &other);
    let durable_dir = fixture.repo.join(".git/modules/lib");
    assert!(!Command::new("git").arg("--git-dir").arg(&durable_dir)
        .args(["cat-file", "-e", &commit]).output().unwrap().status.success());
    fs::remove_dir_all(&other).unwrap();
    let registration = git(&fixture.repo, &["worktree", "list", "--porcelain"]);
    assert!(registration.contains(other.to_str().unwrap()));
    success(fixture.gc("t-retry").output().unwrap());
    assert_eq!(git(&fixture.repo, &["worktree", "list", "--porcelain"]), registration);
    assert!(private_dir.is_dir());
    success(Command::new("git").arg("--git-dir").arg(&private_dir)
        .arg("--work-tree").arg(&fixture.repo)
        .args(["cat-file", "-e", &format!("{commit}^{{commit}}")]).output().unwrap());
    assert_eq!(fixture.certificates("t-retry"), 1);
    assert_eq!(fixture.certificates("t-other"), 0);
}

#[test]
fn failed_worktree_listing_refuses_gc_without_recording_durability() {
    let fixture = Fixture::new();
    fixture.accept("t-list-failure", &fixture.worktree);
    fs::remove_dir_all(&fixture.worktree).unwrap();
    let registration = git(&fixture.repo, &["worktree", "list", "--porcelain"]);
    let real_git = success(Command::new("sh").args(["-c", "command -v git"]).output().unwrap());
    let bin = fixture.root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let shim = bin.join("git");
    fs::write(&shim, "#!/bin/sh\nif [ \"$3\" = worktree ] && [ \"$4\" = list ]; then\n  echo 'injected listing failure' >&2\n  exit 128\nfi\nexec \"$REAL_GIT\" \"$@\"\n").unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let output = fixture.gc("t-list-failure")
        .env("PATH", std::env::join_paths(paths).unwrap()).env("REAL_GIT", real_git)
        .output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("git worktree list failed"), "{stderr}");
    assert!(stderr.contains("injected listing failure"), "{stderr}");
    assert_eq!(fixture.certificates("t-list-failure"), 0);
    assert_eq!(git(&fixture.repo, &["worktree", "list", "--porcelain"]), registration);
}
