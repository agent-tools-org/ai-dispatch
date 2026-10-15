// API-route registry, launch boundary, stream-source and refusal regressions.
// All account material is synthetic and scoped to a temporary aid home.
use super::*;
use crate::routing::keychain::test_keychain::Guard;
use crate::support::paths::{self, AidHomeGuard};

fn setup() -> (tempfile::TempDir, AidHomeGuard) {
    let home = tempfile::tempdir().unwrap();
    let guard = AidHomeGuard::set(home.path());
    std::fs::write(
        paths::config_path(),
        concat!(
            "[[api_route]]\nid = 'claude-api-ming'\nagent = 'claude'\n",
            "keychain_service = 'aid-anthropic-api'\nkeychain_account = 'account-ming'\n",
            "monthly_budget_usd = 200\n",
            "[[api_route]]\nid = 'claude-api-meet'\nagent = 'claude'\n",
            "keychain_service = 'aid-anthropic-api'\nkeychain_account = 'account-meet'\n",
            "monthly_budget_usd = 200\n",
        ),
    )
    .unwrap();
    (home, guard)
}

fn adapter() -> ApiRouteAgent {
    ApiRouteAgent(
        crate::config::api_route::find("claude-api-ming")
            .unwrap()
            .unwrap(),
    )
}

fn opts() -> RunOpts {
    RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    }
}

#[test]
fn routes_resolve_through_registry_batch_and_marker_names() {
    let (_home, _guard) = setup();
    for id in ["claude-api-ming", "claude-api-meet"] {
        assert!(super::super::registry::custom_agent_exists(id));
        let agent = super::super::registry::resolve_custom_agent(id).unwrap();
        assert_eq!(agent.kind(), AgentKind::Custom);
        assert_eq!(agent.rate_limit_name(), Some(id));
        assert!(agent.streaming());
        let cmd = agent.build_command("test", &opts()).unwrap();
        assert_eq!(cmd.get_program(), "claude");
        assert!(cmd.get_args().any(|arg| arg == "--verbose"));
        assert!(cmd.get_envs().all(|(key, _)| key != "ANTHROPIC_API_KEY"));
    }
    let registry = super::super::registry::load_custom_agents();
    assert_ne!(
        registry["claude-api-ming"].provider,
        registry["claude-api-meet"].provider
    );
}

#[test]
fn key_injected_only_at_route_launch_and_unreadable_item_fails_safely() {
    let (_home, _guard) = setup();
    let route = adapter();
    let _fake = Guard::set(route.0.item(), true);
    let mut command = route.build_command("test", &opts()).unwrap();
    inject_key(&route.0.id, &mut command).unwrap();
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
    let mut plain = ClaudeAgent.build_command("test", &opts()).unwrap();
    inject_key("claude", &mut plain).unwrap();
    assert!(plain.get_envs().all(|(key, _)| key != "ANTHROPIC_API_KEY"));
    let mut missing = Command::new("claude");
    let error = inject_key("claude-api-meet", &mut missing).unwrap_err();
    let error = format!("{error:#}");
    assert!(error.contains("infrastructure error: route claude-api-meet"));
    assert!(error.contains("aid-anthropic-api/account-meet"));
    assert!(!error.contains("synthetic-route-test-value"));
    assert!(missing.get_envs().all(|(key, _)| key != "ANTHROPIC_API_KEY"));
}

#[test]
fn billing_source_requires_env_key_and_does_not_affect_subscription() {
    let (_home, _guard) = setup();
    let route = adapter();
    let good = "\x1b]2;title\x07{\"type\":\"system\",\"subtype\":\"init\",\
        \"apiKeySource\":\"ANTHROPIC_API_KEY\",\"model\":\"claude-haiku\"}";
    assert!(stream_failure(&route, good).unwrap().is_none());
    assert!(route.parse_event(&TaskId("t-init".into()), good).is_some());
    for source in ["\"none\"", "null", "\"other\""] {
        let line = format!("{{\"type\":\"system\",\"subtype\":\"init\",\"apiKeySource\":{source}}}");
        assert_eq!(
            stream_failure(&route, &line).unwrap().as_deref(),
            Some("route claude-api-ming is not billed to its API key")
        );
        assert!(
            crate::routing::rate_limit::get_rate_limit_info(&AgentKind::Custom, Some(&route.0.id))
                .unwrap()
                .needs_human
        );
        assert!(stream_failure(&ClaudeAgent, &line).unwrap().is_none());
    }
    assert!(!paths::aid_dir().join("rate-limit-claude").exists());
}

#[test]
fn credit_refusal_holds_only_route_until_next_local_month() {
    let (_home, _guard) = setup();
    let route = adapter();
    let line = r#"{"type":"result","is_error":true,"result":"Credit balance is too low"}"#;
    assert!(stream_failure(&route, line).unwrap().is_some());
    let hold = std::fs::read_to_string(paths::aid_dir().join("rate-limit-claude-api-ming")).unwrap();
    let (_, next) = api_budget::month_bounds(chrono::Local::now()).unwrap();
    assert!(hold.contains(&crate::routing::rate_limit::format_recovery(next.naive_local())));
    assert!(
        crate::routing::rate_limit::dispatch_blocking_hold(&AgentKind::Custom, Some(&route.0.id)).is_some()
    );
    assert!(!paths::aid_dir().join("rate-limit-claude-api-meet").exists());
    assert!(!paths::aid_dir().join("rate-limit-claude").exists());
    let tool = r#"{"type":"user","message":{"content":[{"is_error":true,"content":"billing error"}]}}"#;
    assert!(stream_failure(&route, tool).unwrap().is_none());
}

#[test]
fn error_parsing_never_marks_subscription() {
    let (_home, _guard) = setup();
    let line = r#"{"type":"error","message":"You've hit your limit"}"#;
    assert!(adapter().parse_event(&TaskId("t-refusal".into()), line).is_some());
    assert!(!paths::aid_dir().join("rate-limit-claude").exists());
}

#[test]
fn terminal_prefixed_results_preserve_reported_cost() {
    let (_home, _guard) = setup();
    let line = "\x1b]2;title\x07{\"type\":\"result\",\"is_error\":false,\
        \"total_cost_usd\":0.001235735,\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}";
    let event = adapter().parse_event(&TaskId("t-cost".into()), line).unwrap();
    assert_eq!(event.metadata.unwrap()["cost_usd"].as_f64(), Some(0.001235735));
}

#[test]
fn advised_and_dispatched_api_routes_resolve_the_same_model() {
    let (_home, _guard) = setup();
    let route = adapter().0;
    let config = registry_config(&route);
    let profile = DeclaredTaskProfile {
        difficulty: TaskDifficulty::Simple,
        budget: TaskBudget::Cheap,
        urgency: TaskUrgency::Normal,
        rigor: TaskRigor::Standard,
    };
    let selection = crate::config::SelectionConfig::default();
    let dispatched = super::super::run_model::RunModelInput::declared(
        &route.id,
        AgentKind::Custom,
        Some(&config),
        profile,
        &selection,
    );
    let advised = super::super::run_model::RunModelInput::declared(
        &route.id,
        AgentKind::Claude,
        None,
        profile,
        &selection,
    );
    assert_eq!(
        super::super::run_model::resolve_run_model(&dispatched),
        super::super::run_model::resolve_run_model(&advised)
    );
}

#[test]
fn structured_billing_refusals_match_all_supported_envelopes() {
    for line in [
        r#"{"type":"error","error":{"message":"credit-balance-too-low"}}"#,
        r#"{"type":"assistant","error":"billing_error","message":{"content":[]}}"#,
        r#"{"type":"result","is_error":true,"result":"Insufficient credits"}"#,
    ] {
        assert!(credit_refusal(&serde_json::from_str(line).unwrap()));
    }
    assert!(!credit_refusal(&serde_json::json!({
        "type": "assistant", "message": {"content": [{"text": "billing error"}]}
    })));
}

#[test]
fn credit_refusal_preserves_reported_cost_before_stopping() {
    let (_home, _guard) = setup();
    let store = crate::store::Store::open_memory().unwrap();
    let id = TaskId("t-credit-cost".into());
    store
        .insert_waiting_task(
            id.as_str(),
            "claude-api-ming",
            "test",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    let mut info = CompletionInfo {
        tokens: None,
        status: TaskStatus::Done,
        model: None,
        cost_usd: None,
        exit_code: None,
    };
    let line = r#"{"type":"result","is_error":true,"result":"Credit balance is too low",
        "total_cost_usd":0.05}"#;
    assert!(record_stream_failure(&adapter(), line, &store, &id, &mut info).unwrap());
    assert_eq!(info.cost_usd, Some(0.05));
    assert_eq!(info.status, TaskStatus::Failed);
}
