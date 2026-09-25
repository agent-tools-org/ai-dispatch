// CLI regressions for acceptance after a sibling collects a shared worktree.
// Uses isolated AID_HOME, temporary Git repositories, and persisted task evidence.

mod common;

use rusqlite::{Connection, OptionalExtension, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git(repo: &Path, args: &[&str]) -> String {
    success(
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap(),
    )
}

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    worktree: PathBuf,
    head: String,
    db: Connection,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("aid-wt-accept-")
            .tempdir_in("/tmp")
            .unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.name", "Test User"]);
        git(&repo, &["config", "user.email", "test@example.invalid"]);
        fs::write(repo.join("base"), "base").unwrap();
        git(&repo, &["add", "base"]);
        git(&repo, &["commit", "-m", "base"]);
        let worktree = root.path().join("shared");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "shared",
                worktree.to_str().unwrap(),
            ],
        );
        fs::write(worktree.join("delivery"), "delivered").unwrap();
        git(&worktree, &["add", "delivery"]);
        git(&worktree, &["commit", "-m", "delivery"]);
        let head = git(&worktree, &["rev-parse", "HEAD"]);
        success(
            common::aid_cmd_in(root.path())
                .args(["board", "--json"])
                .output()
                .unwrap(),
        );
        let db = Connection::open(root.path().join("aid.db")).unwrap();
        Self {
            root,
            repo,
            worktree,
            head,
            db,
        }
    }

    fn task(&self, task: &str, head: Option<&str>) {
        self.db
            .execute(
                "INSERT INTO tasks (id, agent, prompt, status, repo_path, worktree_path,
             final_head_sha, final_branch, worktree_branch, parent_task_id, created_at)
             VALUES (?1, 'codex', 'task', 'done', ?2, ?3, ?4, 'shared', 'shared', ?5, ?6)",
                params![
                    task,
                    self.repo.to_str().unwrap(),
                    self.worktree.to_str().unwrap(),
                    head,
                    (task == "t-retry").then_some("t-original"),
                    chrono::Local::now().to_rfc3339()
                ],
            )
            .unwrap();
    }

    fn aid(&self, args: &[&str]) -> Output {
        common::aid_cmd_in(self.root.path())
            .args(args)
            .output()
            .unwrap()
    }

    fn accepted_head(&self, task: &str) -> Option<String> {
        self.db
            .query_row(
                "SELECT accepted_head_sha FROM task_acceptance WHERE task_id = ?1",
                [task],
                |row| row.get(0),
            )
            .optional()
            .unwrap()
    }

    fn collect_original(&self) {
        self.task("t-original", Some(&self.head));
        git(&self.repo, &["merge", "--ff-only", "shared"]);
        success(self.aid(&["accept", "t-original"]));
        success(self.aid(&["gc", "--task", "t-original"]));
        git(&self.repo, &["branch", "-d", "shared"]);
        assert!(!self.worktree.exists());
    }

    fn refusal(&self, expected: &str) {
        let output = self.aid(&["accept", "t-retry"]);
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("worktree") && stderr.contains("missing"),
            "{stderr}"
        );
        assert!(stderr.contains(expected), "{stderr}");
        assert_eq!(self.accepted_head("t-retry"), None);
        assert!(!self.aid(&["gc", "--task", "t-retry"]).status.success());
    }
}

#[test]
fn accept_and_gc_retry_after_original_collected_shared_worktree() {
    let fixture = Fixture::new();
    fixture.task("t-retry", Some(&fixture.head));
    fixture.collect_original();
    success(fixture.aid(&["accept", "t-retry"]));
    assert_eq!(fixture.accepted_head("t-retry"), Some(fixture.head.clone()));
    let stdout = success(fixture.aid(&["gc", "--task", "t-retry"]));
    assert!(stdout.contains("Worktree was already collected"));
    for task in ["t-original", "t-retry"] {
        let count: i64 = fixture
            .db
            .query_row(
                "SELECT COUNT(*) FROM artifact_durability WHERE task_id = ?1",
                [task],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}

#[test]
fn missing_final_commit_cannot_borrow_sibling_acceptance() {
    let fixture = Fixture::new();
    fixture.task("t-retry", None);
    fixture.collect_original();
    fixture.refusal("no recorded final commit");
}

#[test]
fn unreachable_final_commit_refuses_acceptance_after_sibling_gc() {
    let fixture = Fixture::new();
    fs::write(fixture.worktree.join("delivery"), "retry-only").unwrap();
    git(&fixture.worktree, &["commit", "-am", "retry-only"]);
    let retry_head = git(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.task("t-retry", Some(&retry_head));
    git(&fixture.worktree, &["reset", "--hard", &fixture.head]);
    fixture.collect_original();
    fixture.refusal(&format!("Commit {retry_head} for '' has no durable ref"));
}

#[test]
fn absent_final_commit_object_refuses_acceptance() {
    let fixture = Fixture::new();
    let missing = "a".repeat(40);
    fixture.task("t-retry", Some(&missing));
    fixture.collect_original();
    fixture.refusal(&format!(
        "Commit {missing} for '' exists only in disposable storage"
    ));
}

#[test]
fn mutable_ref_is_not_recorded_final_commit_evidence() {
    let fixture = Fixture::new();
    fixture.task("t-retry", Some("main"));
    fixture.collect_original();
    fixture.refusal("no valid recorded final commit SHA");
}

#[test]
fn registered_missing_worktree_refuses_acceptance() {
    let fixture = Fixture::new();
    fixture.task("t-retry", Some(&fixture.head));
    fs::remove_dir_all(&fixture.worktree).unwrap();
    fixture.refusal("still registered");
}

#[test]
fn missing_repository_refuses_acceptance() {
    let fixture = Fixture::new();
    fixture.task("t-retry", Some(&fixture.head));
    fixture.collect_original();
    fixture
        .db
        .execute("UPDATE tasks SET repo_path = NULL WHERE id = 't-retry'", [])
        .unwrap();
    fixture.refusal("no repository for durability proof");
}

#[test]
fn missing_submodule_storage_refuses_acceptance() {
    let fixture = Fixture::new();
    git(
        &fixture.worktree,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},child", fixture.head),
        ],
    );
    git(&fixture.worktree, &["commit", "-m", "gitlink"]);
    let head = git(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.task("t-retry", Some(&head));
    git(
        &fixture.repo,
        &[
            "worktree",
            "remove",
            "--force",
            fixture.worktree.to_str().unwrap(),
        ],
    );
    fixture.refusal("No durable object store for submodule 'child'");
}

#[test]
fn official_guide_documents_missing_worktree_acceptance_proof() {
    let guide = include_str!("../default-skills/aid-guide/references/task-lifecycle.md");
    for term in [
        "recorded final commit and branch",
        "Before recording acceptance",
        "every recursive submodule commit",
        "durable ref refuses acceptance",
        "does not substitute for evidence of this task's final commit",
    ] {
        assert!(guide.contains(term), "acceptance guide missing {term}");
    }
}
