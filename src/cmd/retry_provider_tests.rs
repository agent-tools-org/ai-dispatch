// Retry route checks run after CLI agent overrides, including legacy default rows.
// Uses stored task fixtures and isolated Codex selection without launching commands.
use super::*;
use crate::agent::codex::cli_config;

struct ConfigReset;
impl Drop for ConfigReset {
    fn drop(&mut self) {
        cli_config::set_test_codex_home(None);
    }
}

#[test]
fn retry_agent_override_escapes_drift_but_codex_still_refuses() {
    let home = crate::support::test_env::TestHome::new();
    std::fs::write(home.path().join("config.toml"), "model_provider='Y'").unwrap();
    cli_config::set_test_codex_home(Some(home.path().to_path_buf()));
    let _reset = ConfigReset;
    let store = Store::open_memory().unwrap();
    let mut task = failed_task("t-provider-override");
    task.custom_agent_name = Some("codex-provider:X".into());
    store.insert_task(&task).unwrap();
    let builds = crate::agent::codex::command_builds::count();
    for agent in [None, Some("codex"), Some("opencode")] {
        let mut request = base_retry(task.id.as_str());
        request.agent = agent.map(str::to_string);
        let result = retry_task_to_run_args(&store, &task, request, false);
        if agent == Some("opencode") {
            assert_eq!(result.unwrap().agent_name, "opencode");
        } else {
            assert!(
                result
                    .err()
                    .expect("Codex drift refusal")
                    .to_string()
                    .contains("task recorded codex-provider:X")
            );
        }
    }
    assert_eq!(crate::agent::codex::command_builds::count(), builds);
}

#[test]
fn legacy_default_record_refusal_does_not_claim_the_original_provider() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), "model_provider='X'").unwrap();
    cli_config::set_test_codex_home(Some(home.path().to_path_buf()));
    let _reset = ConfigReset;
    let store = Store::open_memory().unwrap();
    let task = failed_task("t-provider-legacy");
    assert!(task.custom_agent_name.is_none());
    store.insert_task(&task).unwrap();
    let error = RunArgs::for_retry(&store, &task)
        .err()
        .expect("unrecorded provider must refuse");
    let message = error.to_string();
    assert!(
        message.contains(
            "config.toml now selects codex-provider:X; task recorded openai-chatgpt-plan"
        )
    );
    assert!(!message.contains("task ran on"));
}
