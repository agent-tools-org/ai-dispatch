// E2E coverage for read-only enforcement through the real dispatch worker.
// A fake Claude deliberately writes files; settlement must retain the evidence.
// Deps: compiled aid CLI, SQLite, git, tempfile, and a PATH-injected agent.

use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};
use rusqlite::Connection;
use tempfile::TempDir;
mod common;
use common::aid_cmd_in;

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

struct Fixture { home: TempDir, repo: TempDir, bin: TempDir, head: String }

impl Fixture {
    fn new(script: &str) -> Self {
        let home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        let bin = TempDir::new().unwrap();
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.name", "Test"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        std::fs::write(repo.path().join("a.txt"), "original\n").unwrap();
        git(repo.path(), &["add", "a.txt"]);
        git(repo.path(), &["commit", "-m", "initial"]);
        let head = git(repo.path(), &["rev-parse", "HEAD"]);
        let agent = bin.path().join("claude");
        std::fs::write(&agent, format!("#!/bin/sh\ncase \"$1\" in --help|--version) echo 'Claude Code 2.1.285'; exit 0;; esac\n{script}\nprintf '%s\\n' '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"Inspection completed.\"}}'\n")).unwrap();
        std::fs::set_permissions(agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { home, repo, bin, head }
    }

    fn run(&self, background: bool, expected: &str, result: &str) -> Connection {
        let mut cmd = aid_cmd_in(self.home.path());
        cmd.env("PATH", format!("{}:{}", self.bin.path().display(), std::env::var("PATH").unwrap_or_default()))
            .args(["run", "claude", "Inspect the checkout", "--read-only", "--no-audit", "--no-backup", "--id", "t-read-only", "--result-file", result, "--dir"])
            .arg(self.repo.path());
        if background { cmd.arg("--bg"); }
        let output = cmd.output().unwrap();
        assert_eq!(output.status.success(), background || expected == "done", "stdout: {}\nstderr: {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        if background {
            let wait = aid_cmd_in(self.home.path()).args(["wait", "t-read-only", "--timeout", "15"]).output().unwrap();
            assert_eq!(wait.status.success(), expected == "done", "{}", String::from_utf8_lossy(&wait.stderr));
        }
        let db = Connection::open(self.home.path().join("aid.db")).unwrap();
        let status: String = db.query_row("SELECT status FROM tasks WHERE id = 't-read-only'", [], |row| row.get(0)).unwrap();
        assert_eq!(status, expected);
        assert_eq!(git(self.repo.path(), &["rev-parse", "HEAD"]), self.head);
        assert!(git(self.repo.path(), &["diff", "--cached", "--name-only"]).is_empty());
        assert_eq!(git(self.repo.path(), &["stash", "list"]), "");
        assert!(git(self.repo.path(), &["for-each-ref", "refs/aid/"]).is_empty());
        db
    }

    fn assert_error_paths(&self, db: &Connection, paths: &[&str]) {
        let detail: String = db.query_row("SELECT detail FROM events WHERE task_id = 't-read-only' AND event_type = 'error' AND detail LIKE 'Read-only violation:%'", [], |row| row.get(0)).unwrap();
        for path in paths { assert!(detail.contains(path), "{detail}"); }
    }
}

#[test]
fn read_only_stray_file_fails_and_preserves_evidence() {
    let f = Fixture::new("echo stray > probe.txt\necho report > report.md");
    let db = f.run(false, "failed", "report.md");
    f.assert_error_paths(&db, &["probe.txt"]);
    assert_eq!(std::fs::read_to_string(f.repo.path().join("probe.txt")).unwrap(), "stray\n");
}

#[test]
fn read_only_stray_file_fails_in_background() {
    let f = Fixture::new("echo stray > probe.txt\necho report > report.md");
    let db = f.run(true, "failed", "report.md");
    f.assert_error_paths(&db, &["probe.txt"]);
    assert!(f.repo.path().join("probe.txt").exists());
}

#[test]
fn read_only_only_result_file_is_done() {
    let f = Fixture::new("echo report > report.md");
    f.run(false, "done", "report.md");
    assert_eq!(std::fs::read_to_string(f.repo.path().join("report.md")).unwrap(), "report\n");
}

#[test]
fn read_only_only_result_file_is_done_in_background() {
    let f = Fixture::new("mkdir -p reports\necho report > reports/result.md");
    f.run(true, "done", "reports/result.md");
}

#[test]
fn read_only_detects_edits_to_preexisting_dirty_files() {
    let f = Fixture::new("echo agent >> a.txt\necho agent >> notes.txt\necho report > report.md");
    std::fs::write(f.repo.path().join("a.txt"), "user edit\n").unwrap();
    std::fs::write(f.repo.path().join("notes.txt"), "user notes\n").unwrap();
    let db = f.run(false, "failed", "report.md");
    f.assert_error_paths(&db, &["a.txt", "notes.txt"]);
    assert_eq!(std::fs::read_to_string(f.repo.path().join("a.txt")).unwrap(), "user edit\nagent\n");
}

#[test]
fn read_only_detects_deletions() {
    let f = Fixture::new("rm a.txt notes.txt\necho report > report.md");
    std::fs::write(f.repo.path().join("notes.txt"), "user notes\n").unwrap();
    let db = f.run(false, "failed", "report.md");
    f.assert_error_paths(&db, &["a.txt", "notes.txt"]);
    assert!(!f.repo.path().join("a.txt").exists());
}

#[test]
fn read_only_preserves_unchanged_dirty_baseline_and_ignores_aid_paths() {
    let f = Fixture::new("echo report > report.md\necho bookkeeping > .aid-lock");
    std::fs::write(f.repo.path().join("a.txt"), "user edit\n").unwrap();
    std::fs::write(f.repo.path().join("notes.txt"), "user notes\n").unwrap();
    f.run(false, "done", "report.md");
    assert_eq!(std::fs::read_to_string(f.repo.path().join("a.txt")).unwrap(), "user edit\n");
}

#[test]
fn read_only_absolute_result_file_is_done() {
    let f = Fixture::new("echo report > report.md");
    f.run(false, "done", f.repo.path().join("report.md").to_str().unwrap());
}

#[test]
fn read_only_result_like_user_file_is_not_exempt() {
    let f = Fixture::new("echo stray > result-summary.md\necho report > report.md");
    let db = f.run(false, "failed", "report.md");
    f.assert_error_paths(&db, &["result-summary.md"]);
}

#[test]
fn read_only_rename_into_result_file_still_reports_deleted_source() {
    let f = Fixture::new("mv a.txt report.md");
    let db = f.run(false, "failed", "report.md");
    f.assert_error_paths(&db, &["a.txt"]);
    assert!(!f.repo.path().join("a.txt").exists());
    assert!(f.repo.path().join("report.md").exists());
}

#[test]
fn read_only_reports_quoted_and_tabbed_paths() {
    let f = Fixture::new("echo stray > 'odd name.txt'\necho stray > 'tab\tname.txt'\necho report > report.md");
    let db = f.run(false, "failed", "report.md");
    f.assert_error_paths(&db, &["odd name.txt", "tab\tname.txt"]);
}
