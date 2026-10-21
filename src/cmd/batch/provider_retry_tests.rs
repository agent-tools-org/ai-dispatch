// Batch retry overrides and fallback can leave a historical Codex provider route.
// Same-agent retries retain drift refusal; tests never read operator configuration.
use super::*;
use crate::agent::codex::cli_config;

struct ConfigReset;
impl Drop for ConfigReset {
    fn drop(&mut self) {
        cli_config::set_test_codex_home(None);
    }
}

#[test]
fn batch_retry_and_fallback_apply_agent_switch_before_provider_verification() {
    let (home, _home, _cache, store) = setup();
    let config_home = home.path().join("codex");
    std::fs::create_dir(&config_home).unwrap();
    std::fs::write(config_home.join("config.toml"), "model_provider='Y'").unwrap();
    cli_config::set_test_codex_home(Some(config_home));
    let _reset = ConfigReset;
    let mut task = make_stored_task("t-batch-provider", AgentKind::Codex, TaskStatus::Failed);
    task.custom_agent_name = Some("codex-provider:X".into());
    task.effective_dir = Some(home.path().display().to_string());
    store.insert_task(&task).unwrap();
    let mut saved = args(home.path());
    saved.agent_name = "codex".into();
    saved.cascade = vec!["opencode".into()];
    store
        .update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap())
        .unwrap();
    let builds = crate::agent::codex::command_builds::count();
    let switched = retry_task_to_run_args(&store, &task, "wg-provider", Some("opencode")).unwrap();
    assert_eq!(switched.agent_name, "opencode");
    let error = retry_task_to_run_args(&store, &task, "wg-provider", Some("codex"))
        .err()
        .expect("same-agent batch retry must refuse drift");
    assert!(error.to_string().contains("task recorded codex-provider:X"));
    let (_, fallback) = auto_fallback_args(&store, task.id.as_str(), &[], 0)
        .unwrap()
        .unwrap();
    assert_eq!(fallback.agent_name, "opencode");
    assert_eq!(crate::agent::codex::command_builds::count(), builds);
}
