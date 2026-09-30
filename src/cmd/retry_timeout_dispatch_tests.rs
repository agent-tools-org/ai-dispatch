// Manual retry timeout overrides through actual dispatch and worker completion.
// Shares retry fixtures; fake custom agent avoids external provider calls.
use super::*;

#[tokio::test]
async fn idle_override_recomputes_first_token_and_preserves_exact_hard_timeout() {
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(home.path());
    crate::paths::ensure_dirs().unwrap();
    std::fs::create_dir_all(crate::paths::aid_dir().join("agents")).unwrap();
    std::fs::write(crate::paths::aid_dir().join("agents/retry-policy.toml"),
        "[agent]\nid = \"retry-policy\"\ndisplay_name = \"Retry Policy\"\ncommand = \"/bin/sh\"\nprompt_mode = \"arg\"\nfixed_args = [\"-c\", \"echo completed\"]\ninteractive_input = false\n").unwrap();
    let store = std::sync::Arc::new(Store::open_memory().unwrap());
    let mut task = failed_task("t-idle-dispatch");
    task.agent = AgentKind::Custom;
    task.custom_agent_name = Some("retry-policy".into());
    task.requested_model = None;
    let mut saved = RunArgs {
        agent_name: "retry-policy".into(), prompt: task.prompt.clone(),
        idle_timeout_secs: Some(300), timeout: Some(71), read_only: true,
        env: Some(std::collections::HashMap::from([("TOKEN".into(), "do-not-replay".into())])),
        ..Default::default()
    };
    saved.timeout_policy.idle = std::time::Duration::from_secs(300);
    saved.timeout_policy.first_token = std::time::Duration::from_secs(300);
    saved.timeout_policy.max_duration = std::time::Duration::from_secs(71);
    insert_with_saved(&store, &task, &saved);
    let mut retry = base_retry(task.id.as_str());
    retry.idle_timeout_secs = Some(900);
    let retry_id = crate::cmd::retry::retry_task(store.clone(), retry, false).await.unwrap();
    let actual = RunArgs::saved_for_task(&store, retry_id.as_str()).unwrap().unwrap();
    assert_eq!(actual.timeout_policy.idle.as_secs(), 900);
    assert_eq!(actual.timeout_policy.first_token.as_secs(), 900);
    assert_eq!(actual.timeout_policy.max_duration.as_secs(), 71);
    assert!(actual.env.is_none());
    assert_eq!(store.get_task(retry_id.as_str()).unwrap().unwrap().status, TaskStatus::Done);
}
