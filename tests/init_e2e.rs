// E2E coverage for `aid init`.
// Verifies default files are installed and existing customizations are preserved.
// Deps: compiled `aid` binary and tempfile.

use tempfile::TempDir;

mod common;
use common::aid_cmd_in;

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
        "skills/aid-guide/SKILL.md",
        "skills/aid-guide/agents/openai.yaml",
        "skills/aid-guide/references/command-index.md",
        "skills/aid-guide/references/command-errors.md",
        "skills/aid-guide/references/task-lifecycle.md",
        "templates/bug-fix.md",
        "templates/feature.md",
        "templates/refactor.md",
    ] {
        assert!(aid_home.path().join(path).exists(), "missing {path}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Initialized 6 skills and 3 templates"));
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
    assert!(stdout.contains("Initialized 5 skills and 2 templates"));
}

#[test]
fn init_refreshes_release_managed_official_guide() {
    let aid_home = TempDir::new().unwrap();
    let first = aid_cmd_in(aid_home.path()).arg("init").output().unwrap();
    assert!(first.status.success());
    let guide = aid_home.path().join("skills/aid-guide/SKILL.md");
    std::fs::write(&guide, "stale guide").unwrap();

    let second = aid_cmd_in(aid_home.path()).arg("init").output().unwrap();

    assert!(second.status.success());
    let content = std::fs::read_to_string(guide).unwrap();
    assert!(content.contains("name: aid-guide"));
    assert!(!content.contains("stale guide"));
    assert!(String::from_utf8_lossy(&second.stdout).contains("Updated official skill:"));
}
