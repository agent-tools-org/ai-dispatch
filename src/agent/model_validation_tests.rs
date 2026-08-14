// Tests for pre-dispatch model validation.

use super::*;
use std::process::Command;
use std::sync::Mutex;
use crate::agent::{Agent, RunOpts};
use crate::types::*;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

struct MockQueryableAgent {
    kind: AgentKind,
    models: Mutex<Option<Vec<String>>>,
}

impl MockQueryableAgent {
    fn new(kind: AgentKind, models: Option<Vec<String>>) -> Self {
        Self { kind, models: Mutex::new(models) }
    }
}

impl Agent for MockQueryableAgent {
    fn kind(&self) -> AgentKind { self.kind }
    fn streaming(&self) -> bool { false }
    fn accepts_interactive_input(&self) -> bool { false }
    fn build_command(&self, _prompt: &str, _opts: &RunOpts) -> Result<Command> { Ok(Command::new("true")) }
    fn parse_event(&self, _task_id: &TaskId, _line: &str) -> Option<TaskEvent> { None }
    fn served_models(&self) -> Result<Option<Vec<String>>> { Ok(self.models.lock().unwrap().clone()) }
}

#[test]
fn validate_model_allows_valid_model() {
    clear_served_models_cache();
    let mock = MockQueryableAgent::new(AgentKind::Codex, Some(vec!["gpt-5.6-sol".to_string(), "gpt-5.5".to_string()]));

    assert!(validate_model_for_agent(&mock, "gpt-5.6-sol", ModelSource::UserSupplied).is_ok());
    assert!(validate_model_for_agent(&mock, "GPT-5.5", ModelSource::UserSupplied).is_ok());
}

#[test]
fn validate_model_rejects_absent_model_naming_served() {
    clear_served_models_cache();
    let mock = MockQueryableAgent::new(AgentKind::Codex, Some(vec!["gpt-5.6-sol".to_string(), "gpt-5.5".to_string()]));

    let err = validate_model_for_agent(&mock, "auto", ModelSource::UserSupplied)
        .unwrap_err()
        .to_string();
    assert!(err.contains("Agent 'codex' does not serve model 'auto'"));
    assert!(err.contains("Served models: gpt-5.6-sol, gpt-5.5"));
}

#[test]
fn validate_model_allows_unqueryable_cli() {
    clear_served_models_cache();
    let mock = MockQueryableAgent::new(AgentKind::Kilo, None);

    assert!(validate_model_for_agent(&mock, "any-unknown-model", ModelSource::AidResolved).is_ok());
}

#[test]
fn cursor_auto_model_is_allowed() {
    clear_served_models_cache();
    let mock = MockQueryableAgent::new(AgentKind::Cursor, Some(vec![
        "composer-2.5".to_string(),
        "auto".to_string(),
        "default".to_string(),
        "router".to_string(),
    ]));

    assert!(validate_model_for_agent(&mock, "auto", ModelSource::UserSupplied).is_ok());
    assert!(validate_model_for_agent(&mock, "composer-2.5", ModelSource::UserSupplied).is_ok());
    assert!(validate_model_for_agent(&mock, "unserved-model", ModelSource::UserSupplied).is_err());
}

#[test]
fn cursor_probe_failure_returns_none_and_allows_non_alias_models() {
    clear_served_models_cache();
    let mock = MockQueryableAgent::new(AgentKind::Cursor, None);

    assert!(validate_model_for_agent(&mock, "composer-2.5", ModelSource::AidResolved).is_ok());
    assert!(validate_model_for_agent(&mock, "auto", ModelSource::AidResolved).is_ok());
    assert!(validate_model_for_agent(&mock, "custom-model-xyz", ModelSource::AidResolved).is_ok());
}

#[test]
fn agy_real_captured_fixture_parsing_and_rejection() {
    clear_served_models_cache();
    let captured = "\
Fetching available models...
gemini-3.7-flash-high\tGemini 3.7 Flash (High)
gemini-3.7-flash-medium\tGemini 3.7 Flash (Medium)
gemini-3.7-flash-low\tGemini 3.7 Flash (Low)
gemini-3.6-flash-high\tGemini 3.6 Flash (High)
gemini-3.6-flash-medium\tGemini 3.6 Flash (Medium)
gemini-3.6-flash-low\tGemini 3.6 Flash (Low)
gemini-3.5-flash-high\tGemini 3.5 Flash (High)
gemini-3.5-flash-medium\tGemini 3.5 Flash (Medium)
gemini-3.5-flash-low\tGemini 3.5 Flash (Low)
gemini-3.1-pro-high\tGemini 3.1 Pro (High)
gemini-3.1-pro-low\tGemini 3.1 Pro (Low)
claude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)
claude-opus-4-6-thinking\tClaude Opus 4.6 (Thinking)
gpt-oss-120b-medium\tGPT-OSS 120B (Medium)
";
    let models = crate::agent::antigravity::parse_agy_models_output(captured);
    assert!(!models.contains(&"Fetching".to_string()), "Fetching line must not be parsed as a model");
    assert!(models.contains(&"gemini-3.7-flash-high".to_string()));
    assert!(models.contains(&"claude-sonnet-4-6".to_string()));

    let mock = MockQueryableAgent::new(AgentKind::Antigravity, Some(models));

    let res = validate_model_for_agent(&mock, "gemini-9.9-nonexistent", ModelSource::UserSupplied);
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("Agent 'agy' does not serve model 'gemini-9.9-nonexistent'"));
}

#[test]
fn grok_real_captured_fixture_parsing() {
    clear_served_models_cache();
    let captured = "\
You are logged in with grok.com.

Default model: grok-4.6

Available models:
  * grok-4.6 (default)
  - grok-4.5
";
    let models = crate::agent::grok::parse_grok_models_output(captured);
    assert_eq!(models, vec!["grok-4.6".to_string(), "grok-4.5".to_string()]);
}

#[test]
fn served_models_disk_caching_and_clearing() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    clear_served_models_cache();

    let mock = MockQueryableAgent::new(AgentKind::Antigravity, Some(vec!["gemini-3.7-flash-high".to_string()]));

    let models = get_served_models_cached(&mock).expect("models present");
    assert_eq!(models, vec!["gemini-3.7-flash-high".to_string()]);

    let cache_file = cache_file_path();
    assert!(cache_file.exists(), "disk cache file must exist");

    clear_served_models_cache();
    assert!(!cache_file.exists(), "disk cache file must be removed");
}

#[test]
fn validate_slow_cli_probe_success_asserts_no_cannot_query_warning() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    clear_served_models_cache();

    let mock = MockQueryableAgent::new(AgentKind::Antigravity, Some(vec!["gemini-3.7-flash-high".to_string()]));

    let res = validate_model_for_agent(&mock, "gemini-9.9-nonexistent", ModelSource::UserSupplied);
    assert!(res.is_err(), "Must reject unserved user-supplied model");
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("Agent 'agy' does not serve model 'gemini-9.9-nonexistent'"));
    assert!(!err_msg.contains("Cannot query served models"), "Must NOT report timeout / cannot query");
}

#[test]
fn stale_disk_cache_entry_reprobes() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    clear_served_models_cache();

    let cache_file = cache_file_path();
    if let Some(parent) = cache_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let stale_json = serde_json::json!({
        "agy": {
            "models": ["stale-model-v1"],
            "updated_at_secs": 100
        }
    });
    std::fs::write(&cache_file, stale_json.to_string()).expect("write stale cache");

    let mock = MockQueryableAgent::new(AgentKind::Antigravity, Some(vec!["fresh-model-v2".to_string()]));

    let models = get_served_models_cached(&mock).expect("models present");
    assert_eq!(models, vec!["fresh-model-v2".to_string()], "Stale cache must be ignored and re-probed");
}

#[test]
fn fresh_disk_cache_entry_serves_cache() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    clear_served_models_cache();

    let cache_file = cache_file_path();
    if let Some(parent) = cache_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let fresh_json = serde_json::json!({
        "agy": {
            "models": ["cached-model-v1"],
            "updated_at_secs": now_secs()
        }
    });
    std::fs::write(&cache_file, fresh_json.to_string()).expect("write fresh cache");

    let mock = MockQueryableAgent::new(AgentKind::Antigravity, Some(vec!["should-not-be-queried".to_string()]));

    let models = get_served_models_cached(&mock).expect("models present");
    assert_eq!(models, vec!["cached-model-v1".to_string()], "Fresh cache must be returned");
}

#[test]
fn validate_model_refreshes_cache_on_missing_model() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    clear_served_models_cache();

    let mock = MockQueryableAgent::new(AgentKind::Antigravity, Some(vec!["gemini-3.7-flash-high".to_string()]));

    assert_eq!(get_served_models_cached(&mock), Some(vec!["gemini-3.7-flash-high".to_string()]));

    *mock.models.lock().unwrap() = Some(vec!["gemini-3.7-flash-high".to_string(), "gemini-3.8".to_string()]);

    let res = validate_model_for_agent(&mock, "gemini-3.8", ModelSource::UserSupplied);
    assert!(res.is_ok(), "Missing model validation must refresh cache and accept newly added model");
}
