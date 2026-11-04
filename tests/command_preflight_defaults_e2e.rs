// End-to-end checks for run validation after target-project defaults are applied.
// Uses temporary Git projects and dry-run; never dispatches an external agent.
// Deps: common subprocess helpers, tempfile, serde_json, rusqlite.

#[path = "fixtures/aic_shim.rs"]
mod aic_shim;

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
    let bin = home.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    common::write_executable(&bin.join("aic"), &aic_shim::script("exit 0"));
    let path = std::env::join_paths(std::iter::once(bin)
        .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    let output = aid_cmd_in(home.path()).env("PATH", path).args([
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

#[test]
fn explicit_audit_without_aic_is_refused_even_for_dry_run() {
    let home = TempDir::new().unwrap();
    let output = aid_cmd_in(home.path()).env("PATH", home.path()).args([
        "run", "codex", "Inspect the project", "--audit", "--dry-run", "--no-hint",
    ]).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("audit_unavailable"), "{stderr}");
    let db = rusqlite::Connection::open(home.path().join("aid.db")).unwrap();
    let count: i64 = db.query_row("SELECT count(*) FROM tasks", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn project_default_audit_without_aic_warns_and_allows_dry_run() {
    let home = TempDir::new().unwrap();
    let dir = project(&home, "[audit]\nauto = true\n");
    let bin = home.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let git = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("git")).find(|path| path.is_file()).expect("git on PATH");
    std::os::unix::fs::symlink(git, bin.join("git")).unwrap();
    let output = aid_cmd_in(home.path()).env("PATH", &bin).args([
        "run", "codex", "Inspect the project", "--dry-run", "--no-hint", "--dir",
    ]).arg(&dir).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("project audit default") && stderr.contains("aic"), "{stderr}");
}

#[test]
fn audit_identity_status_controls_dispatch_and_default_warnings() {
    for (version, help, status) in [
        ("aic 0.3.2", "AI-powered commit message generator", "Foreign"),
        ("aic 0.3.0", "Cross-audit aid agent outputs", "TooOld(aic 0.3.0)"),
        ("aic 0.10.0", "Cross-audit aid agent outputs", "Ready"),
        ("aic 0.3.2", "Cross-audit aid agent outputs", "Ready"),
    ] {
        for explicit in [false, true] {
            let home = TempDir::new().unwrap();
            let dir = project(&home, "[audit]\nauto = true\n");
            let bin = home.path().join("bin");
            common::write_executable(
                &bin.join("aic"), &aic_shim::identified_script(version, help, "exit 0"),
            );
            let path = std::env::join_paths(std::iter::once(bin)
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
            let mut command = aid_cmd_in(home.path());
            command.env("PATH", path).args([
                "run", "codex", "Inspect the project", "--dry-run", "--no-hint", "--dir",
            ]).arg(&dir);
            if explicit { command.arg("--audit"); }
            let output = command.output().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.success(), !explicit || status == "Ready", "{stderr}");
            if status != "Ready" {
                assert!(stderr.contains(status), "{stderr}");
                assert!(stderr.contains("curl -fsSL https://aid.agent-tools.org/install.sh | sh"));
                if !explicit { assert!(stderr.contains("project audit default")); }
                if status == "Foreign" {
                    assert!(stderr.contains("cargo install aic installs an unrelated project"));
                }
            }
        }
    }
}
