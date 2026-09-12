// End-to-end remote build configuration and task inspection with fake rbox.
// Covers precedence, once-only selection, prelaunch failure and CLI conflicts.
// Deps: compiled aid, tempfile, Bash and git; executes only on remote test hosts.
use std::path::Path;
use std::process::{Command, Output};
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;
mod common;

struct Fixture { temp: TempDir }
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp");
        for dir in ["bin", "repo/.aid", "aid"] {
            std::fs::create_dir_all(temp.path().join(dir)).expect("directory");
        }
        assert!(Command::new("git").args(["init", "-q"]).arg(temp.path().join("repo")).status().expect("git").success());
        std::fs::write(temp.path().join("repo/.aid/project.toml"), "[project]\nid='remote-test'\nremote_build='auto'\n").expect("project");
        script(&temp.path().join("bin/rbox"), "#!/bin/bash\necho pick >> \"$PICKS\"\necho chosen-box\n");
        Self { temp }
    }
    fn command(&self) -> Command {
        let mut cmd = common::aid_cmd_with_cwd(&self.temp.path().join("aid"), &self.temp.path().join("repo"));
        cmd.env("PATH", format!("{}:{}", self.temp.path().join("bin").display(), std::env::var("PATH").expect("path")));
        cmd.env("PICKS", self.temp.path().join("picks"));
        cmd
    }
    fn run(&self, flags: &[&str]) -> Output {
        self.command().args(["run", "codex", "Inspect the project configuration", "--dry-run", "--no-skill", "--no-audit", "--no-hint"])
            .args(flags).output().expect("aid run")
    }
    fn task_json(&self, output: &Output) -> serde_json::Value {
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let id = stdout.lines().find_map(|line| line.strip_prefix("[dry-run] Task: ")).expect("task ID");
        let result = self.command().args(["show", id, "--json"]).output().expect("show");
        assert!(result.status.success(), "{result:?}");
        serde_json::from_slice(&result.stdout).expect("task JSON")
    }
}
fn script(path: &Path, content: &str) {
    std::fs::write(path, content).expect("script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("permissions");
}

#[test]
fn project_auto_picks_once_and_show_json_exposes_box() {
    let f = Fixture::new();
    let json = f.task_json(&f.run(&[]));
    assert_eq!(json["remote_build"], "chosen-box");
    assert_eq!(std::fs::read_to_string(f.temp.path().join("picks")).expect("picks"), "pick\n");
    assert!(json["events"].as_array().expect("events").iter().any(|event| event["metadata"]["remote_build"] == "chosen-box"));
}

#[test]
fn cli_explicit_box_overrides_project_auto_without_picking() {
    let f = Fixture::new();
    let json = f.task_json(&f.run(&["--remote-build", "explicit-box"]));
    assert_eq!(json["remote_build"], "explicit-box");
    assert!(!f.temp.path().join("picks").exists());
}

#[test]
fn no_free_box_preserves_rbox_stderr_before_launch() {
    let f = Fixture::new();
    script(&f.temp.path().join("bin/rbox"), "#!/bin/bash\necho 'no rust-build boxes free' >&2\nexit 1\n");
    let output = f.run(&["--remote-build"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no rust-build boxes free"), "{output:?}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("[dry-run] Task:"));
}

#[test]
fn missing_rbox_is_a_clear_prelaunch_error() {
    let f = Fixture::new();
    let output = f.command().env("PATH", "/nonexistent")
        .args(["run", "codex", "Inspect config", "--dry-run", "--remote-build", "box", "--no-hint"])
        .output().expect("run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("rbox CLI not found"), "{output:?}");
}

#[test]
fn project_remote_build_rejects_sandbox_and_container() {
    let f = Fixture::new();
    for flags in [vec!["--sandbox"], vec!["--container", "image"]] {
        let output = f.run(&flags);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("conflicts with"), "{output:?}");
    }
}
