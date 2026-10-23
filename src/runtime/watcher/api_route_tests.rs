// Process-level proof that an API source mismatch stops before later agent work.
// Uses a shell stream fixture and temporary route state; no real credentials.
use super::super::*;
use crate::support::paths::{self, AidHomeGuard};
use std::process::Stdio;

#[tokio::test]
async fn api_source_mismatch_kills_child_and_records_route_hold() {
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
    let agent = crate::agent::registry::resolve_custom_agent("claude-api-ming").unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let id = TaskId("t-api-source".into());
    super::super::streaming_tests::insert_running_task(&store, &id);
    let mut command = tokio::process::Command::new("/bin/sh");
    command
        .args([
            "-c",
            concat!(
                "printf '\\033]2;title\\007{\"type\":\"system\",\"subtype\":\"init\",",
                "\"apiKeySource\":\"none\"}\n'; sleep 30; touch should-not-run",
            ),
        ])
        .current_dir(home.path())
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    assert!(
        command
            .as_std()
            .get_args()
            .all(|arg| !arg.to_string_lossy().contains('\0'))
    );
    let mut child = command.spawn().unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        watch_streaming_with_input(
            agent.as_ref(),
            &mut child,
            &id,
            &store,
            &home.path().join("agent.log"),
            None,
            Duration::from_secs(60),
            None,
            None,
            None,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.status, TaskStatus::Failed);
    assert!(!home.path().join("should-not-run").exists());
    let events = store.get_events(id.as_str()).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.detail == "route claude-api-ming is not billed to its API key")
    );
    assert!(
        rate_limit::get_rate_limit_info(&AgentKind::Custom, Some("claude-api-ming"))
            .unwrap()
            .needs_human
    );
    assert!(!paths::aid_dir().join("rate-limit-claude").exists());
}
