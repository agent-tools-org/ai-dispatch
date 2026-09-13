// Pins dispatch preflight: unsupported combos fail before a task row exists.
// Deps: prepare_dispatch, Store, RunArgs, isolated AID_HOME.

use super::*;
use super::validate_command_preflight_with;
use std::sync::Arc;

fn isolated_home() -> crate::paths::AidHomeGuard {
    let temp = tempfile::tempdir().unwrap();
    crate::paths::AidHomeGuard::set(temp.path())
}

#[test]
fn prepare_dispatch_accepts_qwen_read_only() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "qwen".to_string(),
        prompt: "Investigate a concrete task routing bug.".to_string(),
        read_only: true,
        existing_task_id: Some(TaskId("t-preflight-ro".to_string())),
        ..Default::default()
    };

    let prepared = super::prepare_dispatch_with(&store, &mut args, |_| true)
        .expect("qwen --read-only should pass command preflight");
    assert_eq!(prepared.task.id.as_str(), "t-preflight-ro");
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
fn validate_command_preflight_rejects_missing_codex_binary() {
    let agent = crate::agent::get_agent(AgentKind::Codex);
    let args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Inspect the repository state carefully.".to_string(),
        ..Default::default()
    };
    let err = validate_command_preflight_with(agent.as_ref(), &args, None, |_| false)
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "Agent 'codex' not found: binary 'codex' missing from PATH"
    );
}

#[test]
fn validate_command_preflight_skips_path_probe_on_dry_run() {
    let agent = crate::agent::get_agent(AgentKind::Codex);
    let args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Inspect the repository state carefully.".to_string(),
        dry_run: true,
        ..Default::default()
    };
    // Dry-run never spawns; missing host binaries must not block the preview.
    validate_command_preflight_with(agent.as_ref(), &args, None, |_| false).unwrap();
}

#[test]
fn prepare_dispatch_rejects_custom_agent_with_missing_binary_before_task_exists() {
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
