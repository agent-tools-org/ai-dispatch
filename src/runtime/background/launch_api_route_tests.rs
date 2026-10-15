// Worker launch injects the API key after shared environment setup.
// Missing credentials stop command preparation before a child can be spawned.
use super::*;
use crate::routing::keychain::test_keychain::Guard;
use crate::support::paths::{self, AidHomeGuard};

#[test]
fn launch_reads_key_after_env_setup_and_fails_before_spawn_if_missing() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    paths::ensure_dirs().unwrap();
    std::fs::write(
        paths::config_path(),
        concat!(
            "[[api_route]]\nid = 'claude-api-ming'\nagent = 'claude'\n",
            "keychain_service = 'test'\nmonthly_budget_usd = 200\n",
        ),
    )
    .unwrap();
    let route = crate::config::api_route::find("claude-api-ming")
        .unwrap()
        .unwrap();
    let item = route.item();
    let agent = crate::agent::api_route::ApiRouteAgent(route);
    let spec: BackgroundRunSpec = serde_json::from_value(serde_json::json!({
        "task_id": "t-api-launch", "agent_name": "claude-api-ming", "prompt": "test",
        "dir": null, "output": null, "verify": null, "retry": 0, "group": null
    }))
    .unwrap();
    let args = RunArgs {
        agent_name: "claude-api-ming".into(),
        dry_run: true,
        sandbox: true,
        ..Default::default()
    };
    let isolated =
        agent::home_isolation::IsolatedHomeGuard::create_from_home(Some(home.path()), Some("t-api-launch"))
            .unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let _fake = Guard::set(item.clone(), true);
    let (command, _) = prepare_launch(&store, &spec, &args, &agent, &isolated).unwrap();
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == "ANTHROPIC_API_KEY" && value.is_some())
    );
    assert!(
        !command
            .get_args()
            .any(|arg| arg.to_string_lossy().contains("synthetic-route-test-value"))
    );
    let _missing = Guard::set(item, false);
    let error = match prepare_launch(&store, &spec, &args, &agent, &isolated) {
        Ok(_) => panic!("unreadable key must fail before launch"),
        Err(error) => format!("{error:#}"),
    };
    assert!(error.contains("infrastructure error: route claude-api-ming"));
    assert!(!error.contains("synthetic-route-test-value"));
}
