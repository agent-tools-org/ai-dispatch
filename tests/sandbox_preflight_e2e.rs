// Exercises Codex capability preflight and launch-time grants through the CLI.
// Uses isolated repositories and a fake Codex executable to capture actual launches.
// Deps: compiled aid binary, tempfile, git, and Unix shell permissions.

#![cfg(unix)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    caller: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let caller = create_repo(root.path(), "caller");
        let target = create_repo(root.path(), "target");
        let bin = root.path().join("bin");
        fs::create_dir(&bin).unwrap();
        fs::create_dir(root.path().join("home")).unwrap();
        fs::write(bin.join("codex"), r#"#!/bin/sh
case "$*" in
  *--version*) echo 'codex-cli 0.154.0'; exit 0 ;;
  *--help*) echo '--approve-for-me'; exit 0 ;;
esac
test -d "$TMPDIR" || exit 1
test ! -f Cargo.toml || test -d "$CARGO_TARGET_DIR" || exit 1
printf '%s\n' "$HOME" "$TMPDIR" "$CARGO_TARGET_DIR" "$@" > "$PREFLIGHT_CAPTURE"
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"NO_CHANGES_NEEDED: launch captured"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}'
"#).unwrap();
        fs::set_permissions(bin.join("codex"), fs::Permissions::from_mode(0o755)).unwrap();
        Self { root, caller, target }
    }

    fn run(&self, cwd: &Path, dir: Option<&Path>, dry_run: bool) -> Output {
        let mut cmd = common::aid_cmd_in(&self.root.path().join("aid"));
        let path = format!("{}:{}", self.root.path().join("bin").display(), std::env::var("PATH").unwrap());
        cmd.current_dir(cwd).env("HOME", self.root.path().join("home"))
            .env("PATH", path)
            .env("CARGO_TARGET_DIR", self.root.path().join("cache"))
            .env("PREFLIGHT_CAPTURE", self.capture())
            .args(["run", "codex", "inspect writable target", "--read-only", "--no-audit", "--timeout", "10"]);
        if let Some(dir) = dir {
            cmd.arg("--dir").arg(dir);
        }
        if dry_run {
            cmd.arg("--dry-run");
        }
        cmd.output().unwrap()
    }

    fn capture(&self) -> PathBuf {
        self.root.path().join("launch.txt")
    }

    fn assert_grants(&self, repo: &Path) {
        let capture = fs::read_to_string(self.capture()).unwrap();
        let mut lines = capture.lines();
        let home = Path::new(lines.next().unwrap());
        let temp = Path::new(lines.next().unwrap());
        let target = Path::new(lines.next().unwrap());
        assert_eq!(temp.parent(), Some(home));
        assert_eq!(target, self.root.path().join("cache/_base").canonicalize().unwrap());
        let config = lines.find(|line| line.starts_with("sandbox_workspace_write.writable_roots=")).unwrap();
        let parsed: toml::Value = config.parse().unwrap();
        let roots = parsed["sandbox_workspace_write"]["writable_roots"].as_array().unwrap();
        for path in [repo.join(".git").canonicalize().unwrap(), temp.into(), target.into()] {
            assert!(roots.contains(&toml::Value::String(path.to_str().unwrap().into())));
        }
        assert_eq!(roots.len(), 3);
    }
}

fn create_repo(root: &Path, name: &str) -> PathBuf {
    let repo = root.join(name);
    fs::create_dir(&repo).unwrap();
    assert!(Command::new("git").args(["init", "-q"]).arg(&repo).status().unwrap().success());
    fs::write(repo.join("Cargo.toml"), "[package]\nname = 'fixture'\nversion = '0.1.0'\n").unwrap();
    repo
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}

#[test]
fn dry_run_ignores_read_only_caller_git_metadata() {
    let fixture = Fixture::new();
    let gitdir = fixture.caller.join(".git");
    fs::set_permissions(&gitdir, fs::Permissions::from_mode(0o555)).unwrap();
    let output = fixture.run(&fixture.caller, Some(&fixture.target), true);
    fs::set_permissions(&gitdir, fs::Permissions::from_mode(0o755)).unwrap();
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("[dry-run] Agent: codex"));
    assert!(!fixture.capture().exists());
    assert!(!fixture.root.path().join("cache").exists());
}

#[test]
fn launch_probes_selected_checkout_and_recreates_deleted_base() {
    let fixture = Fixture::new();
    let gitdir = fixture.caller.join(".git");
    for attempt in 0..2 {
        fs::set_permissions(&gitdir, fs::Permissions::from_mode(0o555)).unwrap();
        let output = fixture.run(&fixture.caller, Some(&fixture.target), false);
        fs::set_permissions(&gitdir, fs::Permissions::from_mode(0o755)).unwrap();
        assert_success(&output);
        fixture.assert_grants(&fixture.target);
        if attempt == 0 {
            fs::remove_dir_all(fixture.root.path().join("cache/_base")).unwrap();
            fs::remove_file(fixture.capture()).unwrap();
        }
    }
}

#[test]
fn launch_without_dir_keeps_current_checkout_and_scratch_grants() {
    let fixture = Fixture::new();
    let output = fixture.run(&fixture.target, None, false);
    assert_success(&output);
    fixture.assert_grants(&fixture.target);
}

#[test]
fn read_only_non_rust_checkout_launches_without_git_grant_and_records_warning() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.target.join("Cargo.toml")).unwrap();
    let gitdir = fixture.target.join(".git");
    fs::set_permissions(&gitdir, fs::Permissions::from_mode(0o555)).unwrap();
    let output = fixture.run(&fixture.caller, Some(&fixture.target), false);
    fs::set_permissions(&gitdir, fs::Permissions::from_mode(0o755)).unwrap();
    assert_success(&output);
    let capture = fs::read_to_string(fixture.capture()).unwrap();
    let config = capture.lines().find(|line| line.starts_with("sandbox_workspace_write.writable_roots=")).unwrap();
    let parsed: toml::Value = config.parse().unwrap();
    let roots = parsed["sandbox_workspace_write"]["writable_roots"].as_array().unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].as_str(), capture.lines().nth(1));
    let conn = rusqlite::Connection::open(fixture.root.path().join("aid/aid.db")).unwrap();
    let warning: String = conn.query_row(
        "SELECT detail FROM events WHERE detail LIKE 'Omitting writable Git metadata grant%'",
        [], |row| row.get(0),
    ).unwrap();
    assert!(warning.contains(gitdir.to_str().unwrap()), "{warning}");
    assert!(warning.contains("directory is read-only"), "{warning}");
}

#[test]
fn launch_refuses_read_only_owned_target_before_agent_starts() {
    let fixture = Fixture::new();
    let target = fixture.root.path().join("cache/_base");
    fs::create_dir_all(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o555)).unwrap();
    let output = fixture.run(&fixture.caller, Some(&fixture.target), false);
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("cannot write to granted directory"), "{error}");
    assert!(error.contains(target.to_str().unwrap()), "{error}");
    assert!(!fixture.capture().exists());
}
