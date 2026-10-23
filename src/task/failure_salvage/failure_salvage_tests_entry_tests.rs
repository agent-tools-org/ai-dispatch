// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

#[test]
fn salvage_excludes_aid_directory_from_commit() {
    let (_home, _guard) = isolated_home();
    let repo = init_repo();
    std::fs::write(repo.path().join("base.txt"), "changed\n").expect("write");
    std::fs::create_dir_all(repo.path().join(".aid")).expect("mkdir");
    std::fs::write(repo.path().join(".aid/state.toml"), "health = 1\n").expect("write state");
    let store = Store::open_memory().expect("store");
    let mut task = task("t-salvage-aid-dir", Some(repo.path().display().to_string()));
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("insert");

    crate::task::lifecycle::mark_failed(&store, &task.id).expect("fail");

    let committed = git_stdout(
        repo.path(),
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    );
    assert!(
        committed.lines().any(|line| line == "base.txt"),
        "got: {committed}"
    );
    assert!(
        !committed.lines().any(|line| line == ".aid/state.toml"),
        "got: {committed}"
    );
    // The file stays on disk (untouched), just never staged/committed by aid.
    assert!(repo.path().join(".aid/state.toml").exists());
}

#[test]
fn salvage_error_does_not_change_failed_status() {
    let (_home, _guard) = isolated_home();
    let not_repo = tempfile::tempdir().expect("tempdir");
    let store = Store::open_memory().expect("store");
    let mut task = task("t-salvage-error", Some(not_repo.path().display().to_string()));
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("insert");

    crate::task::lifecycle::mark_failed(&store, &task.id).expect("fail");

    let loaded = store.get_task(task.id.as_str()).expect("load").expect("task");
    assert_eq!(loaded.status, TaskStatus::Failed);
}
