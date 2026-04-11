// Tests for worktree dependency preparation helpers.
// Exports: none.
// Deps: super helpers, crate::store, tempfile.

use super::*;
use tempfile::TempDir;

fn task_id() -> TaskId {
    TaskId("t-worktree-deps".to_string())
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn setup_runs_once_and_writes_marker() {
    let store = Store::open_memory().unwrap();
    let repo = TempDir::new().unwrap();
    let worktree = TempDir::new().unwrap();
    let setup_file = worktree.path().join("setup-count.txt");
    prepare_worktree_dependencies(
        &store,
        &task_id(),
        repo.path(),
        worktree.path(),
        Some("printf ran >> setup-count.txt"),
        true,
        Some(60),
        true,
    )
    .unwrap();
    prepare_worktree_dependencies(
        &store,
        &task_id(),
        repo.path(),
        worktree.path(),
        Some("printf ran >> setup-count.txt"),
        true,
        Some(60),
        false,
    )
    .unwrap();

    assert_eq!(fs::read_to_string(setup_file).unwrap(), "ran");
    assert!(worktree.path().join(SETUP_DONE_MARKER).exists());
}

#[test]
fn symlink_fallback_links_node_modules_when_package_json_exists() {
    let store = Store::open_memory().unwrap();
    let repo = TempDir::new().unwrap();
    let worktree = TempDir::new().unwrap();
    write(&repo.path().join("frontend/package.json"), "{}");
    write(&repo.path().join("frontend/node_modules/pkg/index.js"), "module.exports = 1;\n");

    prepare_worktree_dependencies(
        &store,
        &task_id(),
        repo.path(),
        worktree.path(),
        None,
        true,
        None,
        true,
    )
    .unwrap();

    let linked = worktree.path().join("frontend/node_modules");
    assert!(linked.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(linked.canonicalize().unwrap(), repo.path().join("frontend/node_modules").canonicalize().unwrap());
}

#[test]
fn symlink_fallback_is_skipped_when_setup_is_defined() {
    let store = Store::open_memory().unwrap();
    let repo = TempDir::new().unwrap();
    let worktree = TempDir::new().unwrap();
    write(&repo.path().join("frontend/package.json"), "{}");
    write(&repo.path().join("frontend/node_modules/pkg/index.js"), "module.exports = 1;\n");

    prepare_worktree_dependencies(
        &store,
        &task_id(),
        repo.path(),
        worktree.path(),
        Some("printf ok > setup.log"),
        true,
        Some(60),
        true,
    )
    .unwrap();

    assert!(!worktree.path().join("frontend/node_modules").exists());
    assert!(worktree.path().join(SETUP_DONE_MARKER).exists());
}

#[test]
fn link_deps_false_disables_symlink_fallback() {
    let store = Store::open_memory().unwrap();
    let repo = TempDir::new().unwrap();
    let worktree = TempDir::new().unwrap();
    write(&repo.path().join("frontend/package.json"), "{}");
    write(&repo.path().join("frontend/node_modules/pkg/index.js"), "module.exports = 1;\n");

    prepare_worktree_dependencies(
        &store,
        &task_id(),
        repo.path(),
        worktree.path(),
        None,
        false,
        None,
        true,
    )
    .unwrap();

    assert!(!worktree.path().join("frontend/node_modules").exists());
}

#[test]
fn missing_deps_hint_requires_fresh_worktree_without_setup_or_links() {
    let worktree = TempDir::new().unwrap();
    write_verify_state(worktree.path(), true, false, false).unwrap();
    assert_eq!(missing_deps_hint(worktree.path()), Some(MISSING_DEPS_HINT));
    write_verify_state(worktree.path(), true, true, false).unwrap();
    assert_eq!(missing_deps_hint(worktree.path()), None);
}
