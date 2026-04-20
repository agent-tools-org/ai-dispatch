// Tests for background agent binary preflight behavior.
// Covers missing built-in agent binaries before command construction.
// Deps: background run specs and AgentKind parsing.

use super::{ensure_agent_binary_available_with, BackgroundRunSpec};

#[test]
fn background_preflight_rejects_missing_kilo_binary() {
    let spec = BackgroundRunSpec {
        agent_name: "kilo".to_string(),
        ..make_spec("t-kilo")
    };

    let err = ensure_agent_binary_available_with(&spec, |_| false).unwrap_err();

    assert_eq!(
        err.to_string(),
        "Agent 'kilo' not found: binary missing from PATH"
    );
}

#[test]
fn background_preflight_skips_containerized_runs() {
    let spec = BackgroundRunSpec {
        agent_name: "kilo".to_string(),
        container: Some("ubuntu:latest".to_string()),
        ..make_spec("t-kilo")
    };

    ensure_agent_binary_available_with(&spec, |_| false).unwrap();
}

fn make_spec(task_id: &str) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: task_id.to_string(),
        worker_pid: None,
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: Some(".".to_string()),
        output: None,
        result_file: None,
        model: None,
        verify: None,
        setup: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        max_duration_mins: None,
        idle_timeout_secs: None,
        retry: 0,
        group: None,
        skills: vec![],
        checklist: vec![],
        template: None,
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: None,
        sandbox: false,
        read_only: false,
        container: None,
        link_deps: false,
        pre_task_dirty_paths: None,
    }
}
