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
fn refuses_local_state_without_deleting_it() {
    let temp = tempfile::tempdir().unwrap();
    let task = temp.path().join("task");
    let mirror = task.join("codex-home");
    let real = temp.path().join("real");
    std::fs::create_dir_all(real.join("sessions")).unwrap();
    std::fs::write(real.join("state_5.sqlite"), "synthetic state").unwrap();
    let home = Home::create(&real, &task).unwrap();
    std::fs::create_dir(home.path.join("cache")).unwrap();
    std::fs::write(home.path.join("cache/nested"), "cached").unwrap();
    for name in [
        "thread_history_1.sqlite-wal",
        "models_cache.json",
        "thread_history_1.sqlite",
    ] {
        std::fs::write(home.path.join(name), name).unwrap();
    }
    std::fs::write(home.path.join("config.toml.bak"), "mirror config").unwrap();
    std::os::unix::fs::symlink(real.join("missing"), home.path.join("stale")).unwrap();
    let before = tree_snapshot(temp.path());
    let error = Home::create(&real, &task).err().unwrap().to_string();
    assert_eq!(
        error,
        format!(
            "Codex mirror contains local state invisible to the real home: \
         cache, models_cache.json, thread_history_1.sqlite, thread_history_1.sqlite-wal. \
         With no aid Codex run active, move these entries from {} into {}, \
         or run Codex once directly with that CODEX_HOME before using aid.",
            mirror.display(),
            real.canonicalize().unwrap().display()
        )
    );
    assert_eq!(tree_snapshot(temp.path()), before);
}

fn tree_snapshot(root: &Path) -> Vec<(PathBuf, std::fs::FileType, Vec<u8>)> {
    let mut snapshot = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let kind = entry.file_type().unwrap();
        let data = if kind.is_symlink() {
            std::fs::read_link(&path)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec()
        } else if kind.is_file() {
            std::fs::read(&path).unwrap()
        } else {
            Vec::new()
        };
        if kind.is_dir() {
            snapshot.extend(tree_snapshot(&path));
        }
        snapshot.push((path, kind, data));
    }
    snapshot.sort_by(|left, right| left.0.cmp(&right.0));
    snapshot
}

#[test]
fn missing_real_state_names_initialization_command_without_changing_home() {
    for missing in ["sessions", "state_5.sqlite"] {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        std::fs::create_dir_all(real.join("sessions")).unwrap();
        std::fs::write(real.join("state_5.sqlite"), "synthetic state").unwrap();
        let task = temp.path().join("task");
        std::fs::create_dir(&task).unwrap();
        if missing == "sessions" {
            std::fs::remove_dir(real.join(missing)).unwrap();
        } else {
            std::fs::remove_file(real.join(missing)).unwrap();
        }
        let before = tree_snapshot(temp.path());
        let error = Home::create(&real, &task).err().unwrap().to_string();
        assert!(error.contains("requires existing sessions and state_5.sqlite"));
        assert!(error.contains(&format!(
            "start `codex app-server` once (or run Codex once) with CODEX_HOME={}",
            real.canonicalize().unwrap().display()
        )));
        assert_eq!(tree_snapshot(temp.path()), before);
    }
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
    std::fs::remove_file(real.join("removed")).unwrap();
    Home::create(&real, temp.path()).unwrap();
    assert!(home.path.join("removed").symlink_metadata().is_err());
}

#[test]
fn removes_only_regular_mirror_config_files() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    std::fs::create_dir_all(real.join("sessions")).unwrap();
    std::fs::write(real.join("state_5.sqlite"), "synthetic state").unwrap();
    std::fs::write(real.join("config.toml"), "real config").unwrap();
    let home = Home::create(&real, temp.path()).unwrap();
    for name in ["config.toml", "config.toml.bak", "config.toml-old"] {
        std::fs::write(home.path.join(name), "Codex trust config").unwrap();
    }
    Home::create(&real, temp.path()).unwrap();
    assert!(std::fs::read_dir(&home.path).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("config.toml")
    }));
    let link = home.path.join("config.toml-link");
    std::os::unix::fs::symlink(real.join("config.toml"), &link).unwrap();
    assert!(Home::create(&real, temp.path()).is_err());
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(
        std::fs::read_to_string(real.join("config.toml")).unwrap(),
        "real config"
    );
}
