// Worker and retry refusal when Codex configuration drifts from persisted identity.
// Historical display uses only the row even when live configuration becomes unreadable.
use super::*;

struct ConfigReset;
impl Drop for ConfigReset {
    fn drop(&mut self) {
        agent::codex::cli_config::set_test_codex_home(None);
    }
}

#[test]
fn switched_provider_refuses_worker_and_retry_before_building_y_argv() {
    let home = tempfile::tempdir().unwrap();
    let _aid = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().unwrap();
    let config = home.path().join("config.toml");
    std::fs::write(&config, "model_provider='X'").unwrap();
    agent::codex::cli_config::set_test_codex_home(Some(home.path().to_path_buf()));
    let _reset = ConfigReset;
    let (store, spec, args, task) = setup_worker(home.path());
    let adapter = lifecycle_agent_for_task(&store, &spec, &args).unwrap();
    assert_eq!(adapter.rate_limit_name(), Some("codex-provider:X"));
    std::fs::write(&config, "model_provider='Y'").unwrap();
    let isolated = agent::home_isolation::IsolatedHomeGuard::create_from_home(
        Some(home.path()),
        Some("t-drift"),
    )
    .unwrap();
    let error =
        launch::prepare_launch(&store, &spec, &args, adapter.as_ref(), &isolated).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("config.toml now selects codex-provider:Y; task ran on codex-provider:X")
    );
    assert!(
        RunArgs::for_retry(&store, &task)
            .unwrap_err()
            .to_string()
            .contains("task ran on codex-provider:X")
    );
    assert!(!home.path().join("capture.jsonl").exists());
    std::fs::write(config, "model_provider = [malformed").unwrap();
    assert_eq!(task.route().provider.as_str(), "codex-provider:X");
    assert!(task.display_route().starts_with("codex/codex-provider:X/"));
}

fn setup_worker(home: &std::path::Path) -> (Arc<Store>, BackgroundRunSpec, RunArgs, crate::types::Task) {
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .insert_waiting_task(
            "t-drift", "codex", "prompt", None, None, None, None, None, None, false, false,
        )
        .unwrap();
    store
        .db()
        .execute(
            "UPDATE tasks SET custom_agent_name='codex-provider:X' WHERE id='t-drift'",
            [],
        )
        .unwrap();
    let spec: BackgroundRunSpec = serde_json::from_value(serde_json::json!({
        "task_id":"t-drift", "agent_name":"codex", "prompt":"prompt",
        "dir":home.to_str(), "output":null, "model":null,
        "verify":null, "retry":0, "group":null, "interactive":false,
    }))
    .unwrap();
    let args = RunArgs {
        agent_name: "codex".into(),
        dir: spec.dir.clone(),
        ..Default::default()
    };
    let task = store.get_task("t-drift").unwrap().unwrap();
    (store, spec, args, task)
}
