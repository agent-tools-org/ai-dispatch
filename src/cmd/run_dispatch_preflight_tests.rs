// Pins dispatch preflight: unsupported combos fail before a task row exists.
// Deps: prepare_dispatch, Store, RunArgs, isolated AID_HOME.

use super::*;
use super::validate_command_preflight_with;
use std::ffi::OsString;
use std::sync::Arc;

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: test-only; restored on drop so nested aid sessions stay intact.
        unsafe { std::env::remove_var(key) };
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

fn isolated_home() -> crate::paths::AidHomeGuard {
    let temp = tempfile::tempdir().unwrap();
    crate::paths::AidHomeGuard::set(temp.path())
}

/// prepare_dispatch reads AID_TASK_ID / AID_TASK_DEPTH; clear them so nested
/// outer aid sessions cannot turn capability probes into depth refusals.
fn clear_nested_dispatch_env() -> (EnvVarGuard, EnvVarGuard) {
    (
        EnvVarGuard::remove("AID_TASK_ID"),
        EnvVarGuard::remove("AID_TASK_DEPTH"),
    )
}

#[test]
fn prepare_dispatch_rejects_qwen_read_only_before_task_exists() {
    let _nest = clear_nested_dispatch_env();
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "qwen".to_string(),
        prompt: "Investigate a concrete task routing bug.".to_string(),
        read_only: true,
        existing_task_id: Some(TaskId("t-preflight-ro".to_string())),
        ..Default::default()
    };

    let err = match prepare_dispatch(&store, &mut args) {
        Ok(_) => panic!("qwen --read-only must be refused before task creation"),
        Err(err) => err.to_string(),
    };
    assert!(
        err.contains("does not support read-only mode"),
        "unexpected error: {err}"
    );
    assert!(
        err.contains("omit --read-only"),
        "error should name the remedy: {err}"
    );
    assert!(
        store.get_task("t-preflight-ro").unwrap().is_none(),
        "unsupported dispatch must not create a task row"
    );
}

#[test]
fn validate_command_preflight_accepts_codex_read_only_with_result_file() {
    let agent = crate::agent::get_agent(AgentKind::Codex);
    let args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Audit the module and write findings.".to_string(),
        read_only: true,
        result_file: Some("result.md".to_string()),
        ..Default::default()
    };
    // Inject PATH probe: this test covers command shape, not host install state.
    validate_command_preflight_with(agent.as_ref(), &args, None, |_| true).unwrap();
}

#[test]
fn validate_command_preflight_rejects_missing_resolved_binary() {
    let agent = crate::agent::get_agent(AgentKind::Kilo);
    let args = RunArgs {
        agent_name: "kilo".to_string(),
        prompt: "Implement a focused change with enough context.".to_string(),
        ..Default::default()
    };
    let err = validate_command_preflight_with(agent.as_ref(), &args, None, |_| false)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("binary 'kilo' missing from PATH"),
        "unexpected error: {err}"
    );
}

#[test]
fn prepare_dispatch_rejects_custom_agent_with_missing_binary_before_task_exists() {
    let _nest = clear_nested_dispatch_env();
    let temp = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let agents_dir = crate::paths::aid_dir().join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join("goose.toml"),
        r#"
[agent]
id = "goose"
display_name = "Goose"
command = "definitely-not-on-path-goose-bin"
"#,
    )
    .unwrap();

    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "goose".to_string(),
        prompt: "Investigate a concrete dispatch failure path.".to_string(),
        existing_task_id: Some(TaskId("t-preflight-goose".to_string())),
        ..Default::default()
    };

    let err = match prepare_dispatch(&store, &mut args) {
        Ok(_) => panic!("missing custom binary must be refused before task creation"),
        Err(err) => err.to_string(),
    };
    assert!(
        err.contains("binary 'definitely-not-on-path-goose-bin' missing from PATH"),
        "unexpected error: {err}"
    );
    assert!(
        store.get_task("t-preflight-goose").unwrap().is_none(),
        "missing-binary dispatch must not create a task row"
    );
}
