// End-to-end coverage for `[project] stage` through the compiled aid binary.
// Checks the persisted dry-run prompt of a read-only audit and the warning for an invalid stage.
// Deps: compiled aid binary, tempfile, rusqlite, Git CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

mod common;

const AUDIT: &str = "Read-only audit of the parser change. Do not edit files.";

fn repo(root: &TempDir, stage: &str) -> (PathBuf, PathBuf) {
    let home = root.path().join("home");
    let repo = root.path().join("repo");
    std::fs::create_dir_all(home.join("skills")).unwrap();
    std::fs::create_dir_all(repo.join(".aid")).unwrap();
    assert!(Command::new("git").args(["init", "--quiet"]).arg(&repo).status().unwrap().success());
    std::fs::write(
        repo.join(".aid/project.toml"),
        format!("[project]\nid = \"staged\"\nstage = \"{stage}\"\nrules = [\"staged-rule-marker\"]\n"),
    )
    .unwrap();
    (home, repo)
}

fn dry_run(home: &Path, repo: &Path, id: &str) -> Output {
    common::aid_cmd_with_cwd(home, repo)
        .args(["run", "grok", AUDIT, "--read-only", "--dry-run", "--id", id])
        .output()
        .unwrap()
}

fn resolved_prompt(home: &Path, id: &str) -> String {
    rusqlite::Connection::open(home.join("aid.db")).unwrap()
        .query_row("SELECT resolved_prompt FROM tasks WHERE id = ?1", [id], |row| row.get(0))
        .unwrap()
}

#[test]
fn prototype_stage_lowers_the_audit_bar_in_the_resolved_prompt() {
    let root = TempDir::new().unwrap();
    let (home, repo) = repo(&root, "prototype");
    let output = dry_run(&home, &repo, "t-stage-proto");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let prompt = resolved_prompt(&home, "t-stage-proto");
    assert!(prompt.contains("only a high-impact finding justifies FIX or BLOCK"), "{prompt}");
    assert!(prompt.contains("Project stage: prototype (idea or draft)."), "{prompt}");
    assert!(prompt.contains("- Project stage: prototype. Favor reaching"), "{prompt}");
    assert!(prompt.contains("staged-rule-marker"), "{prompt}");
}

#[test]
fn invalid_stage_warns_instead_of_silently_dropping_the_config() {
    let root = TempDir::new().unwrap();
    let (home, repo) = repo(&root, "production");
    let output = common::aid_cmd_with_cwd(&home, &repo).args(["project", "show"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[aid] Warning: ignoring invalid"), "{stderr}");
    assert!(stderr.contains("project.toml"), "{stderr}");
    assert!(stderr.contains("unknown variant `production`"), "{stderr}");
    assert_eq!(stderr.matches("ignoring invalid").count(), 1, "{stderr}");
}
