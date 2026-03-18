// E2E coverage for `aid init`.
// Verifies default files are installed and existing customizations are preserved.
// Deps: compiled `aid` binary and tempfile.

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn aid_cmd_in(aid_home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aid"));
    cmd.env("AID_HOME", aid_home);
    cmd.env("AID_NO_DETACH", "1");
    cmd
}

#[test]
fn init_creates_default_skills_and_templates() {
    let aid_home = TempDir::new().unwrap();
    let output = aid_cmd_in(aid_home.path()).arg("init").output().unwrap();
    assert!(output.status.success());
    for path in [
        "skills/implementer.md",
        "skills/researcher.md",
        "skills/code-scout.md",
        "skills/debugger.md",
        "skills/test-writer.md",
        "templates/bug-fix.md",
        "templates/feature.md",
        "templates/refactor.md",
    ] {
        assert!(aid_home.path().join(path).exists(), "missing {path}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Initialized 5 skills and 3 templates"));
}

#[test]
fn init_skips_existing_files_without_overwriting_them() {
    let aid_home = TempDir::new().unwrap();
    std::fs::create_dir_all(aid_home.path().join("skills")).unwrap();
    std::fs::create_dir_all(aid_home.path().join("templates")).unwrap();
    std::fs::write(
        aid_home.path().join("skills/implementer.md"),
        "# Custom skill",
    )
    .unwrap();
    std::fs::write(
        aid_home.path().join("templates/feature.md"),
        "# Custom template",
    )
    .unwrap();
    let output = aid_cmd_in(aid_home.path()).arg("init").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(aid_home.path().join("skills/implementer.md")).unwrap(),
        "# Custom skill"
    );
    assert_eq!(
        std::fs::read_to_string(aid_home.path().join("templates/feature.md")).unwrap(),
        "# Custom template"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Skipped existing skill:"));
    assert!(stdout.contains("Skipped existing template:"));
    assert!(stdout.contains("Initialized 4 skills and 2 templates"));
}
