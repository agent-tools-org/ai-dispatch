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

    fn foreground(&self, cwd: &Path, agent: &str, extra: &[&str], expected: &str) -> Connection {
        let output = aid_cmd_in(self.home.path()).current_dir(cwd)
            .env("PATH", format!("{}:{}", self.bin.path().display(), std::env::var("PATH").unwrap_or_default()))
            .args(["run", agent, "Inspect the checkout", "--read-only", "--no-audit", "--no-backup", "--id", "t-read-only"])
            .args(extra).output().unwrap();
        assert_eq!(output.status.success(), expected == "done", "stdout: {}\nstderr: {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let db = Connection::open(self.home.path().join("aid.db")).unwrap();
        let status: String = db.query_row("SELECT status FROM tasks WHERE id = 't-read-only'", [], |row| row.get(0)).unwrap();
        assert_eq!(status, expected);
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

#[test]
fn read_only_relative_run_directory_allows_absolute_result_path() {
    let f = Fixture::new("echo report > report.md");
    let report = f.repo.path().join("report.md");
    let output = aid_cmd_in(f.home.path())
        .current_dir(f.repo.path())
        .env("PATH", format!("{}:{}", f.bin.path().display(), std::env::var("PATH").unwrap_or_default()))
        .args(["run", "claude", "Inspect the checkout", "--dir", ".", "--read-only", "--no-audit", "--no-backup", "--id", "t-read-only", "--result-file"])
        .arg(&report).output().unwrap();
    assert!(output.status.success(), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let db = Connection::open(f.home.path().join("aid.db")).unwrap();
    let status: String = db.query_row("SELECT status FROM tasks WHERE id = 't-read-only'", [], |row| row.get(0)).unwrap();
    assert_eq!(status, "done");
    assert_eq!(std::fs::read_to_string(report).unwrap(), "report\n");
    assert_eq!(git(f.repo.path(), &["rev-parse", "HEAD"]), f.head);
}

#[test]
fn read_only_result_symlink_does_not_exempt_its_destination() {
    let f = Fixture::new("ln -s a.txt report.md\necho altered > report.md");
    let db = f.run(false, "failed", "report.md");
    f.assert_error_paths(&db, &["a.txt"]);
    assert_eq!(std::fs::read_to_string(f.repo.path().join("a.txt")).unwrap(), "altered\n");
}

#[test]
fn read_only_without_dir_guards_gemini_cwd() {
    let f = Fixture::new("echo stray > probe.txt");
    std::os::unix::fs::symlink(f.bin.path().join("claude"), f.bin.path().join("gemini")).unwrap();
    let db = f.foreground(f.repo.path(), "gemini", &["-o", "output.md"], "failed");
    f.assert_error_paths(&db, &["probe.txt"]);
}

#[test]
fn read_only_without_dir_guards_repo_subdirectory() {
    let f = Fixture::new("echo stray > probe.txt");
    let subdir = f.repo.path().join("subdir");
    std::fs::create_dir(&subdir).unwrap();
    let db = f.foreground(&subdir, "claude", &["-o", "output.md"], "failed");
    f.assert_error_paths(&db, &["probe.txt"]);
}

#[test]
fn read_only_non_git_directory_warns_and_proceeds() {
    let f = Fixture::new("echo report > report.md");
    let dir = TempDir::new().unwrap();
    let db = f.foreground(dir.path(), "claude", &["--result-file", "report.md"], "done");
    let warning: String = db.query_row("SELECT detail FROM events WHERE task_id = 't-read-only' AND detail LIKE 'Warning: read-only enforcement unavailable%'", [], |row| row.get(0)).unwrap();
    assert!(warning.contains("not a git repository"), "{warning}");
    assert!(warning.contains(dir.path().to_str().unwrap()), "{warning}");
    assert!(dir.path().join("report.md").exists());
}

#[test]
fn read_only_task_output_file_is_exempt() {
    let f = Fixture::new("");
    f.foreground(f.home.path(), "claude", &["--dir", f.repo.path().to_str().unwrap(), "-o", f.repo.path().join("output.md").to_str().unwrap()], "done");
    assert!(f.repo.path().join("output.md").exists());
}

#[test]
fn read_only_output_symlink_does_not_exempt_destination() {
    let f = Fixture::new("");
    std::os::unix::fs::symlink("a.txt", f.repo.path().join("output.md")).unwrap();
    let db = f.foreground(f.repo.path(), "claude", &["-o", "output.md"], "failed");
    f.assert_error_paths(&db, &["a.txt"]);
}

fn nested_repository(f: &Fixture, tracked: bool) -> std::path::PathBuf {
    let nested = f.repo.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "-b", "main"]);
    git(&nested, &["config", "user.name", "Test"]);
    git(&nested, &["config", "user.email", "test@example.com"]);
    std::fs::write(nested.join("file.txt"), "initial").unwrap();
    git(&nested, &["add", "file.txt"]);
    git(&nested, &["commit", "-m", "initial"]);
    if tracked {
        git(f.repo.path(), &["submodule", "add", "./nested", "nested"]);
        git(f.repo.path(), &["submodule", "absorbgitdirs"]);
        git(f.repo.path(), &["commit", "-am", "gitlink"]);
    }
    nested
}

#[test]
fn read_only_unchanged_embedded_repositories_and_gitlinks_pass() {
    for tracked in [false, true] {
        let f = Fixture::new("");
        nested_repository(&f, tracked);
        f.foreground(f.repo.path(), "claude", &["-o", "output.md"], "done");
    }
}

#[test]
fn read_only_nested_head_changes_fail() {
    for tracked in [false, true] {
        let f = Fixture::new("git -C nested commit --allow-empty -m changed");
        nested_repository(&f, tracked);
        let db = f.foreground(f.repo.path(), "claude", &["-o", "output.md"], "failed");
        f.assert_error_paths(&db, &["nested"]);
    }
}

#[test]
fn read_only_git_failure_fails_before_agent_execution() {
    let f = Fixture::new("echo executed > probe.txt");
    let wrapper = f.bin.path().join("git");
    std::fs::write(&wrapper, "#!/bin/sh\nif [ \"$1\" = status ]; then echo 'fatal: permission denied' >&2; exit 128; fi\nexec /usr/bin/git \"$@\"\n").unwrap();
    std::fs::set_permissions(wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    f.foreground(f.repo.path(), "claude", &["-o", "output.md"], "failed");
    assert!(!f.repo.path().join("probe.txt").exists());
}

#[test]
fn read_only_many_files_are_hashed_in_one_batch_per_snapshot() {
    let f = Fixture::new("");
    for index in 0..2000 {
        std::fs::write(f.repo.path().join(format!("file-{index}.txt")), "unchanged").unwrap();
    }
    let counter = f.home.path().join("hashes");
    let wrapper = f.bin.path().join("git");
    std::fs::write(&wrapper, format!("#!/bin/sh\nif [ \"$1\" = hash-object ]; then echo hash >> '{}'; fi\nexec /usr/bin/git \"$@\"\n", counter.display())).unwrap();
    std::fs::set_permissions(wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    f.foreground(f.repo.path(), "claude", &["-o", "output.md"], "done");
    assert_eq!(std::fs::read_to_string(counter).unwrap().lines().count(), 2);
}

#[test]
fn read_only_output_in_caller_cwd_does_not_exempt_same_name_in_run_dir() {
    let f = Fixture::new("echo stray > output.md");
    let db = f.foreground(f.home.path(), "claude", &["--dir", f.repo.path().to_str().unwrap(), "-o", "output.md"], "failed");
    f.assert_error_paths(&db, &["output.md"]);
    assert!(f.home.path().join("output.md").exists());
}
