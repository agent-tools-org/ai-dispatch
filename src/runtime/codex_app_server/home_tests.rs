// Codex home isolation tests using synthetic files only.
// Covers exclusions, shared writes, symlink identity, and persistent idempotent launches.

use super::*;

#[test]
fn mirrors_entries_idempotently_without_deleting_shared_paths() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    let task = temp.path().join("task");
    std::fs::create_dir(&real).unwrap();
    std::fs::create_dir(real.join("sessions")).unwrap();
    std::fs::write(real.join("state_5.sqlite"), "synthetic state").unwrap();
    for name in [
        "config.toml",
        "config.toml.bak",
        "config.toml-old",
        "auth.json",
        "AGENTS.md",
        "hooks.json",
    ] {
        std::fs::write(real.join(name), "synthetic").unwrap();
    }
    for name in ["skills", "rules"] {
        std::fs::create_dir(real.join(name)).unwrap();
    }
    let path = Home::create(&real, &task).unwrap().path;
    assert!(!path.join("config.toml").exists());
    assert!(!path.join("config.toml.bak").exists());
    assert!(!path.join("config.toml-old").exists());
    assert_mirrors(&path, &real);
    std::fs::write(path.join("auth.json"), "refreshed synthetic").unwrap();
    std::fs::write(path.join("sessions/new"), "session").unwrap();
    assert_eq!(
        std::fs::read_to_string(real.join("auth.json")).unwrap(),
        "refreshed synthetic"
    );
    assert!(path.exists());
    Home::create(&real, &task).unwrap();
    assert_mirrors(&path, &real);
    assert!(real.join("sessions/new").exists());
    assert!(real.join("auth.json").exists());
}

#[test]
fn preserves_local_state_created_in_the_persistent_mirror() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path().join("task");
    std::fs::create_dir_all(task.join("codex-home")).unwrap();
    std::fs::write(task.join("codex-home/keep"), "keep").unwrap();
    let real = temp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::create_dir(real.join("sessions")).unwrap();
    std::fs::write(real.join("state_5.sqlite"), "synthetic state").unwrap();
    Home::create(&real, &task).unwrap();
    assert!(task.join("codex-home/keep").exists());
}

fn assert_mirrors(path: &Path, real: &Path) {
    for name in [
        "auth.json",
        "AGENTS.md",
        "hooks.json",
        "sessions",
        "state_5.sqlite",
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

#[test]
fn concurrent_launches_share_links_and_refresh_new_entries() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    let aid = temp.path().join("aid");
    std::fs::create_dir(&real).unwrap();
    std::fs::create_dir(real.join("sessions")).unwrap();
    std::fs::write(real.join("state_5.sqlite"), "synthetic state").unwrap();
    std::fs::write(real.join("auth.json"), "synthetic").unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| Home::create(&real, &aid).unwrap());
        }
    });
    std::fs::create_dir(real.join("rules")).unwrap();
    Home::create(&real, &aid).unwrap();
    assert_eq!(
        std::fs::read_link(aid.join("codex-home/rules")).unwrap(),
        real.join("rules")
    );
}

#[test]
fn refuses_conflicting_links_and_a_mirror_that_is_the_real_home() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::create_dir(real.join("sessions")).unwrap();
    std::fs::write(real.join("state_5.sqlite"), "synthetic state").unwrap();
    std::fs::write(real.join("auth.json"), "synthetic").unwrap();
    let home = Home::create(&real, temp.path()).unwrap();
    std::fs::remove_file(home.path.join("auth.json")).unwrap();
    std::os::unix::fs::symlink(temp.path().join("wrong"), home.path.join("auth.json")).unwrap();
    assert!(Home::create(&real, temp.path()).is_err());
    assert!(Home::create(&home.path, temp.path()).is_err());
}

#[test]
fn refresh_removes_links_to_deleted_real_entries() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    std::fs::create_dir_all(real.join("sessions")).unwrap();
    std::fs::write(real.join("state_5.sqlite"), "synthetic state").unwrap();
    std::fs::write(real.join("removed"), "synthetic").unwrap();
    let home = Home::create(&real, temp.path()).unwrap();
    std::fs::write(home.path.join("local"), "keep").unwrap();
    std::fs::remove_file(real.join("removed")).unwrap();
    Home::create(&real, temp.path()).unwrap();
    assert!(home.path.join("removed").symlink_metadata().is_err());
    assert_eq!(
        std::fs::read_to_string(home.path.join("local")).unwrap(),
        "keep"
    );
}
