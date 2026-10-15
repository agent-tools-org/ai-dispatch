// Codex home isolation tests using synthetic files only.
// Covers exclusions, shared writes, symlink identity, and guard cleanup.

use super::*;

#[test]
fn mirrors_every_entry_except_user_config_and_cleans_up() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    let task = temp.path().join("task");
    std::fs::create_dir(&real).unwrap();
    for name in [
        "config.toml",
        "config.toml.bak",
        "auth.json",
        "AGENTS.md",
        "hooks.json",
    ] {
        std::fs::write(real.join(name), "synthetic").unwrap();
    }
    for name in ["sessions", "skills", "rules"] {
        std::fs::create_dir(real.join(name)).unwrap();
    }
    let home = Home::create(&real, &task).unwrap();
    let path = home.path.clone();
    assert!(!path.join("config.toml").exists());
    assert!(!path.join("config.toml.bak").exists());
    assert_mirrors(&path, &real);
    std::fs::write(path.join("auth.json"), "refreshed synthetic").unwrap();
    std::fs::write(path.join("sessions/new"), "session").unwrap();
    assert_eq!(
        std::fs::read_to_string(real.join("auth.json")).unwrap(),
        "refreshed synthetic"
    );
    drop(home);
    assert!(!path.exists());
    assert!(real.join("sessions/new").exists());
    assert!(real.join("auth.json").exists());
}

#[test]
fn refuses_to_replace_an_existing_task_home() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path().join("task");
    std::fs::create_dir_all(task.join("codex-home")).unwrap();
    std::fs::write(task.join("codex-home/keep"), "keep").unwrap();
    assert!(Home::create(temp.path(), &task).is_err());
    assert!(task.join("codex-home/keep").exists());
}

fn assert_mirrors(path: &Path, real: &Path) {
    for name in [
        "auth.json",
        "AGENTS.md",
        "hooks.json",
        "sessions",
        "skills",
        "rules",
    ] {
        assert!(
            std::fs::symlink_metadata(path.join(name))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_link(path.join(name)).unwrap(),
            real.join(name)
        );
    }
}
