// Worker launch preservation tests.
// Uses saved run arguments and isolated homes from the launch module.

use super::*;

#[test]
fn worker_launch_preservation_uses_resolved_prompt_with_raw_lifecycle_args() {
    let (home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let id = "t-launch-preservation";
    let resolved = "Resolved launch context\nCaller brief";
    store
        .insert_waiting_task(
            id,
            "codex",
            "Caller brief",
            Some(resolved),
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .update_task_dispatch_args(id, &RunArgs::default().dispatch_args_json().unwrap())
        .unwrap();
    let spec = serde_json::from_value(serde_json::json!({
        "task_id": id, "worker_pid": null, "agent_name": "codex", "prompt": "Stale spec",
        "dir": home.path().to_str(), "output": null, "model": null, "verify": null,
        "retry": 0, "group": null, "interactive": false,
    }))
    .unwrap();
    let args = RunArgs::for_worker(&store, &spec).unwrap();
    let agent = capture_agent();
    let isolated =
        agent::home_isolation::IsolatedHomeGuard::create_from_home(Some(home.path()), Some(id))
            .unwrap();
    let (command, _) = prepare_launch(&store, &spec, &args, &agent, &isolated).unwrap();
    assert_eq!(args.prompt, "Caller brief");
    assert_eq!(command.get_args().last().unwrap(), resolved);
}
fn capture_agent() -> agent::custom::CustomAgent {
    agent::custom::CustomAgent {
        config: agent::custom::parse_config(concat!(
            "[agent]\nid = \"capture\"\ndisplay_name = \"Capture\"\n",
            "command = \"/bin/echo\"\nprompt_mode = \"arg\"\n"
        ))
        .unwrap(),
    }
}

#[test]
fn fix_round_one_read_only_launch_stages_explicit_report_and_output_separately() {
    let temp = crate::support::test_env::TestHome::new();
    let spec: BackgroundRunSpec = serde_json::from_value(serde_json::json!({
        "task_id": "t-routing", "agent_name": "codex", "prompt": "audit",
        "dir": null, "output": null, "verify": null, "retry": 0, "group": null
    }))
    .unwrap();
    let staging = agent::scratch::create_temp_dir(temp.path()).unwrap();
    let args = RunArgs {
        agent_name: "codex".into(),
        read_only: true,
        artifact_staging: Some(staging.display().to_string()),
        output: Some("/outside/final.txt".into()),
        result_file: Some("reports/explicit.md".into()),
        ..Default::default()
    };
    let opts = run_opts(&spec, &args, crate::types::AgentKind::Codex);
    let task = std::path::Path::new(args.artifact_staging.as_ref().unwrap());
    assert_eq!(
        opts.result_file.as_deref(),
        task.join("agent-result.md").to_str()
    );
    assert_eq!(
        opts.output.as_deref(),
        task.join("agent-output.txt").to_str()
    );
    assert_eq!(args.result_file.as_deref(), Some("reports/explicit.md"));
    assert_eq!(args.output.as_deref(), Some("/outside/final.txt"));
}
