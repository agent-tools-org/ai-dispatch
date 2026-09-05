// End-to-end checks for run validation after target-project defaults are applied.
// Uses temporary Git projects and dry-run; never dispatches an external agent.
// Deps: common subprocess helpers, tempfile, serde_json, rusqlite.

mod common;
use common::aid_cmd_in;
use tempfile::TempDir;

fn project(home: &TempDir, defaults: &str) -> std::path::PathBuf {
    let dir = home.path().join("project");
    std::fs::create_dir_all(dir.join(".aid")).unwrap();
    let output = std::process::Command::new("git").args(["init", "-q"])
        .arg(&dir).output().unwrap();
    assert!(output.status.success());
    std::fs::write(dir.join(".aid/project.toml"), format!("[project]\nid = 'test'\n{defaults}")).unwrap();
    dir
}

#[test]
fn project_container_conflict_is_rejected_before_task_creation() {
    let home = TempDir::new().unwrap();
    let dir = project(&home, "container = 'test-image'\n");
    let output = aid_cmd_in(home.path()).args([
        "run", "codex", "Inspect the project", "--sandbox", "--no-hint", "--dry-run", "--dir",
    ]).arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("sandbox_container"), "{stderr}");
    let db = rusqlite::Connection::open(home.path().join("aid.db")).unwrap();
    let count: i64 = db.query_row("SELECT count(*) FROM tasks", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn critical_accepts_enabled_proof_from_project_defaults() {
    let home = TempDir::new().unwrap();
    let dir = project(&home, "verify = 'true'\n[audit]\nauto = true\n");
    let output = aid_cmd_in(home.path()).args([
        "run", "codex", "Inspect the verification pipeline", "--rigor", "critical",
        "--no-hint", "--dry-run", "--dir",
    ]).arg(&dir).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!home.path().join("logs/command-errors.jsonl").exists());
}

#[test]
fn read_only_dir_keeps_the_explicit_checkout_as_the_task_target() {
    let home = TempDir::new().unwrap();
    let dir = project(&home, "");
    let output = aid_cmd_in(home.path()).args([
        "run", "codex", "Inspect the scan lifecycle and report findings", "--kind", "debugging",
        "--read-only", "--no-hint", "--dry-run", "--dir",
    ]).arg(&dir).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let db = rusqlite::Connection::open(home.path().join("aid.db")).unwrap();
    let (effective_dir, read_only): (String, bool) = db.query_row(
        "SELECT effective_dir, read_only FROM tasks", [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(std::path::Path::new(&effective_dir), dir);
    assert!(read_only);
}
