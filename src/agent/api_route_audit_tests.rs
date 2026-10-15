// Cached identity, safe environment, provider-only refusals, and live budget regressions.
// Reuses synthetic API accounts; never accesses a keychain password.
use super::*;

#[test]
fn config_edits_do_not_reclassify_cached_or_unrelated_custom_streams() {
    let (_home, _guard) = setup();
    let cached = crate::agent::registry::resolve_custom_agent("claude-api-ming").unwrap();
    let custom = crate::agent::custom::CustomAgent {
        config: crate::agent::custom::parse_config(
            "[agent]\nid = 'other'\ndisplay_name = 'Other'\ncommand = 'other'",
        )
        .unwrap(),
    };
    std::fs::write(paths::config_path(), "invalid = [").unwrap();
    let store = crate::store::Store::open_memory().unwrap();
    let mut info = CompletionInfo {
        tokens: None,
        status: TaskStatus::Done,
        model: None,
        cost_usd: None,
        exit_code: None,
    };
    let id = TaskId("t-cached".into());
    let line = r#"{"type":"system","subtype":"init","apiKeySource":"apiKeyHelper"}"#;
    assert!(!record_stream_failure(cached.as_ref(), line, &store, &id, &mut info).unwrap());
    assert!(!record_stream_failure(&custom, line, &store, &id, &mut info).unwrap());
    assert!(
        cached
            .build_command("test", &opts())
            .unwrap()
            .get_args()
            .any(|arg| arg == "--settings")
    );
}

#[test]
fn removed_routes_never_resolve_a_plain_claude_placeholder() {
    let (_home, _guard) = setup();
    let cached = crate::agent::registry::resolve_custom_agent("claude-api-ming").unwrap();
    std::fs::write(paths::config_path(), "").unwrap();
    assert!(crate::agent::registry::resolve_custom_agent("claude-api-ming").is_none());
    let command = cached.build_command("test", &opts()).unwrap();
    assert!(command.get_args().any(|arg| arg == "--settings"));
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == "ANTHROPIC_API_KEY" && value.is_none())
    );
}

#[test]
fn route_only_scrubs_all_provider_and_billing_redirects() {
    let (_home, _guard) = setup();
    let route = adapter();
    let mut command = route.build_command("test", &opts()).unwrap();
    let mut plain = ClaudeAgent.build_command("test", &opts()).unwrap();
    for name in ROUTE_ENV_REMOVALS {
        command.env(name, "synthetic");
        plain.env(name, "synthetic");
    }
    route.finalize_launch_env(&mut command);
    ClaudeAgent.finalize_launch_env(&mut plain);
    for name in ROUTE_ENV_REMOVALS {
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key == name && value.is_none())
        );
        assert!(
            plain
                .get_envs()
                .any(|(key, value)| key == name && value.is_some())
        );
    }
}

#[test]
fn prose_and_tool_inputs_do_not_create_monthly_billing_holds() {
    let (_home, _guard) = setup();
    let route = adapter();
    for line in [
        r#"{"type":"result","is_error":true,"result":"Fixed the billing error handler"}"#,
        r#"{"type":"assistant","error":"unrelated_error","message":{"content":[{"text":"billing_error"}]}}"#,
        r#"{"type":"tool_use","error":{"type":"billing_error"},"input":{"command":"billing_error"}}"#,
    ] {
        assert!(stream_failure(&route, line).unwrap().is_none());
    }
    assert!(!paths::aid_dir().join("rate-limit-claude-api-ming").exists());
    for error in [
        "billing_error",
        "credit_balance_too_low",
        "credit-balance-too-low",
    ] {
        let line = serde_json::json!({"type":"assistant", "error":error}).to_string();
        assert!(stream_failure(&route, &line).unwrap().is_some());
    }
}

#[test]
fn assistant_credit_refusal_retains_estimated_spend_without_result() {
    let (_home, _guard) = setup();
    let route = adapter();
    let store = crate::store::Store::open_memory().unwrap();
    let id = TaskId("t-credit-usage".into());
    store
        .insert_waiting_task(
            id.as_str(),
            &route.0.id,
            "test",
            None,
            None,
            None,
            None,
            Some("sonnet"),
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
    let line = r#"{"type":"assistant","error":"billing_error","message":{"id":"credit",
        "model":"sonnet","usage":{"input_tokens":1000000,"output_tokens":10}}}"#;
    assert!(record_stream_failure(&route, line, &store, &id, &mut info).unwrap());
    assert!(info.cost_usd.is_some_and(|cost| cost > 0.0));
    assert!(api_budget::for_route(&route.0, Some(&store)).unwrap().spent > 0.0);
}

#[test]
fn aggregate_live_spend_stops_a_route_even_before_a_result() {
    let (_home, _guard) = setup();
    let route = adapter();
    let store = crate::store::Store::open_memory().unwrap();
    for id in ["t-other", "t-live"] {
        store
            .insert_waiting_task(
                id,
                &route.0.id,
                "test",
                None,
                None,
                None,
                None,
                Some("sonnet"),
                None,
                false,
                false,
            )
            .unwrap();
    }
    store
        .db()
        .execute("UPDATE tasks SET cost_usd = 199 WHERE id = 't-other'", [])
        .unwrap();
    let mut info = CompletionInfo {
        tokens: None,
        status: TaskStatus::Done,
        model: None,
        cost_usd: None,
        exit_code: None,
    };
    let line = r#"{"type":"assistant","message":{"id":"live","model":"sonnet",
        "usage":{"input_tokens":1000000}}}"#;
    assert!(record_stream_failure(&route, line, &store, &TaskId("t-live".into()), &mut info).unwrap());
    assert_eq!(info.status, TaskStatus::Failed);
    assert!(
        store
            .latest_error("t-live")
            .unwrap()
            .contains("monthly budget exhausted")
    );
}

#[test]
fn result_at_exact_remaining_budget_finishes_without_an_excess_cost_failure() {
    let (_home, _guard) = setup();
    let route = adapter();
    let store = crate::store::Store::open_memory().unwrap();
    let id = TaskId("t-at-limit".into());
    store
        .insert_waiting_task(
            id.as_str(),
            &route.0.id,
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
    let line = r#"{"type":"result","is_error":false,"total_cost_usd":200}"#;
    assert!(!record_stream_failure(&route, line, &store, &id, &mut info).unwrap());
    assert_eq!(
        api_budget::for_route(&route.0, Some(&store))
            .unwrap()
            .billing
            .state,
        crate::routing::quota::billing::BillingState::Exhausted
    );
}
