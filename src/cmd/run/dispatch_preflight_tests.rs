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

/// A foreign `agent` on PATH (xAI's Grok Build CLI) and no `cursor-agent`:
/// advise, detect_agents and run preflight must refuse Cursor with one reason.
#[test]
fn foreign_agent_binary_blocks_cursor_in_advise_and_preflight_alike() {
    let _permit = crate::test_subprocess::acquire();
    let bin_dir = tempfile::tempdir().unwrap();
    let agent = bin_dir.path().join("agent");
    std::fs::write(&agent, "#!/bin/sh\necho 'Grok Build TUI'\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
    let helper = "cmd::run::run_dispatch_prepare::preflight_tests::reports_cursor_route_for_subprocess";
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", helper, "--ignored", "--nocapture"])
        .env("PATH", bin_dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "helper failed: {stdout}{}", String::from_utf8_lossy(&output.stderr));
    let marker = |key: &str| {
        stdout.lines().find_map(|line| line.strip_prefix(key)).unwrap_or("<missing>").to_string()
    };
    let reason = "binary 'cursor-agent' missing from PATH";
    assert_eq!(marker("ADVISE_INSTALLED="), "false");
    assert_eq!(marker("ADVISE_ELIGIBLE="), "false");
    assert_eq!(marker("ADVISE_REASON="), format!("not installed: {reason}"));
    assert_eq!(marker("DETECTED="), "false");
    assert_eq!(marker("PREFLIGHT="), format!("Agent 'cursor' not found: {reason}"));
}

#[test]
#[ignore]
fn reports_cursor_route_for_subprocess() {
    let temp = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    let declared = crate::types::DeclaredTaskProfile {
        difficulty: crate::types::TaskDifficulty::Moderate,
        budget: crate::types::TaskBudget::Standard,
        urgency: crate::types::TaskUrgency::Normal,
        rigor: crate::types::TaskRigor::Standard,
    };
    let report = crate::agent::selection::advise("refactor the parser", declared, None, None, None, 0, None);
    let cursor = report.candidates.iter().find(|item| item.agent == "cursor").unwrap();
    println!("ADVISE_INSTALLED={}", cursor.installed);
    println!("ADVISE_ELIGIBLE={}", cursor.eligible);
    let reason = cursor.exclusion_reason.clone().unwrap_or_default();
    println!("ADVISE_REASON={}", reason.split("; ").next().unwrap_or_default());
    println!("DETECTED={}", crate::agent::detect_agents().contains(&AgentKind::Cursor));
    let agent = crate::agent::get_agent(AgentKind::Cursor);
    let args = RunArgs {
        agent_name: "cursor".to_string(),
        prompt: "Implement a focused change with enough context.".to_string(),
        ..Default::default()
    };
    let err = validate_command_preflight_with(agent.as_ref(), &args, None, crate::agent::env::which_exists)
        .unwrap_err();
    println!("PREFLIGHT={err}");
}
