// Real Git and persisted task fixtures for raw patch CLI tests.
// Keeps the database outside the task checkout and commits fixture baselines.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

pub struct Fixture {
    pub home: TempDir,
    pub repo: PathBuf,
    pub db: rusqlite::Connection,
    pub base: String,
}

pub fn git(dir: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    output.stdout
}

pub fn commit(dir: &Path, message: &str) -> String {
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", message]);
    String::from_utf8(git(dir, &["rev-parse", "HEAD"])).unwrap().trim().to_string()
}

impl Fixture {
    pub fn new() -> Self {
        let home = TempDir::new().unwrap();
        let repo = home.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Test"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        for name in ["base.txt", "sibling.txt", "Cargo.lock"] {
            std::fs::write(repo.join(name), "baseline\n").unwrap();
        }
        let base = commit(&repo, "baseline");
        let output = crate::common::aid_cmd_in(home.path()).args(["board", "--all", "--json"])
            .output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let db = rusqlite::Connection::open(home.path().join("aid.db")).unwrap();
        Self { home, repo, db, base }
    }

    pub fn task(&self, id: &str, checkout: &Path, worktree: bool, parent: Option<&str>, base: Option<&str>) {
        self.db.execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at, completed_at, repo_path,
             effective_dir, worktree_path, parent_task_id, start_sha, final_head_sha)
             VALUES (?1, 'codex', 'patch task', 'done', '2026-11-03T00:00:00Z',
             '2026-11-03T00:01:00Z', ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![id, self.repo.to_str(), checkout.to_str(),
                if worktree { checkout.to_str() } else { None }, parent, base],
        ).unwrap();
    }

    pub fn patch(&self, id: &str) -> Output {
        crate::common::aid_cmd_in(self.home.path()).args(["show", id, "--patch"]).output().unwrap()
    }

    pub fn bytes(&self, id: &str) -> Vec<u8> {
        let output = self.patch(id);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        output.stdout
    }

    pub fn worktree(&self) -> PathBuf {
        let path = self.home.path().join("worktree");
        git(&self.repo, &["worktree", "add", "-qb", "delivery", path.to_str().unwrap()]);
        path
    }
}
