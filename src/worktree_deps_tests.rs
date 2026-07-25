// Tests for worktree dependency preparation helpers.
// Exports: none.
// Deps: super helpers, crate::store, tempfile.

use super::*;
use crate::types::{AgentKind, Task, TaskStatus, VerifyStatus};
use chrono::Local;
use std::ffi::{OsStr, OsString};
use tempfile::TempDir;

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let previous = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

fn task_id() -> TaskId {
    TaskId("t-worktree-deps".to_string())
}

fn make_task(id: TaskId) -> Task {
    Task {
        id,
        agent: AgentKind::Codex,
        custom_agent_name: None, prompt: "test prompt".to_string(), resolved_prompt: None,
        status: TaskStatus::Running, category: None, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None,
        repo_path: None, worktree_path: None, worktree_branch: None, final_head_sha: None,
        final_branch: None, start_sha: None, log_path: None, output_path: None, tokens: None,
        prompt_tokens: None, duration_ms: None, model: None, cost_usd: None, exit_code: None,
        created_at: Local::now(), completed_at: None, verify: None,
        verify_status: VerifyStatus::Skipped, pending_reason: None, read_only: false,
        budget: false, audit_verdict: None, audit_report_path: None, delivery_assessment: None,
    }
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
        None,
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
        None,
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
        None,
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
        None,
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
        None,
    )
    .unwrap();

    assert!(!worktree.path().join("frontend/node_modules").exists());
}

#[test]
fn rust_worktree_setup_records_cargo_target_seed() {
    let store = Store::open_memory().unwrap();
    let id = task_id();
    store.insert_task(&make_task(id.clone())).unwrap();
    let cache = TempDir::new().unwrap();
    let _target_dir = EnvVarGuard::set("CARGO_TARGET_DIR", cache.path().join("_base"));
    let repo = TempDir::new().unwrap();
    let worktree = TempDir::new().unwrap();
    let source = cache.path().join("_base/debug");
    write(&worktree.path().join("Cargo.toml"), "[package]\nname = \"demo\"\n");
    write(&source.join("artifact.txt"), "cached");

    prepare_worktree_dependencies(
        &store,
        &id,
        repo.path(),
        worktree.path(),
        None,
        false,
        None,
        true,
        Some("feat/shared-cache"),
    )
    .unwrap();

    let target = cache.path().join("feat-shared-cache/debug/artifact.txt");
    let events = store.get_events(id.as_str()).unwrap();
    assert_eq!(fs::read_to_string(target).unwrap(), "cached");
    let details = events.iter().map(|event| event.detail.as_str()).collect::<Vec<_>>();
    assert!(
        details.iter().any(|detail| detail.contains("Cargo target seeded")),
        "missing seed event in {details:?}"
    );
}

#[test]
fn missing_deps_hint_requires_fresh_worktree_without_setup_or_links() {
    let worktree = TempDir::new().unwrap();
    write_verify_state(worktree.path(), true, false, false).unwrap();
    assert_eq!(missing_deps_hint(worktree.path()), Some(MISSING_DEPS_HINT));
    write_verify_state(worktree.path(), true, true, false).unwrap();
    assert_eq!(missing_deps_hint(worktree.path()), None);
}
