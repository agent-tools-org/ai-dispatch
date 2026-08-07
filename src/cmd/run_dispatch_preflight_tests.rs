// Pins dispatch preflight: unsupported combos fail before a task row exists.
// Deps: prepare_dispatch, Store, RunArgs, isolated AID_HOME.

use super::*;
use super::validate_command_preflight;
use std::sync::Arc;

fn isolated_home() -> crate::paths::AidHomeGuard {
    let temp = tempfile::tempdir().unwrap();
    crate::paths::AidHomeGuard::set(temp.path())
}

#[test]
fn prepare_dispatch_rejects_qwen_read_only_before_task_exists() {
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
    validate_command_preflight(agent.as_ref(), &args, None).unwrap();
}
