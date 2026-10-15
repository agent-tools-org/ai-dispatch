// Cached identity, safe environment, provider-only refusals, and launch-only budget regressions.
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
    for &name in ROUTE_ENV_REMOVALS {
        command.env(name, "synthetic");
        plain.env(name, "synthetic");
    }
    route.finalize_launch_env(&mut command);
    ClaudeAgent.finalize_launch_env(&mut plain);
    for &name in ROUTE_ENV_REMOVALS {
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
fn malformed_config_keeps_ordinary_custom_agents_resolvable() {
    let (_home, _guard) = setup();
    let agents = paths::aid_dir().join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("other.toml"),
        "[agent]\nid = 'other'\ndisplay_name = 'Other'\ncommand = 'other'",
    )
    .unwrap();
    std::fs::write(paths::config_path(), "invalid = [").unwrap();
    let custom = crate::agent::registry::resolve_custom_agent("other").unwrap();
    assert!(crate::routing::rate_limit::dispatch_blocking_hold(&custom.kind(), Some("other")).is_none());
    assert!(crate::agent::registry::resolve_custom_agent("claude-api-ming").is_none());
    let error = crate::config::load_config().unwrap_err().to_string();
    assert!(!error.is_empty());
}

#[test]
fn route_settings_keep_permissions_but_no_disk_env_or_helper_override() {
    let (_home, _guard) = setup();
    let dir = paths::aid_dir().join(".claude");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("settings.json"),
        serde_json::json!({
            "env":{"ANTHROPIC_BASE_URL":"https://example.invalid"},
            "apiKeyHelper":"malicious", "permissions":{"deny":["Bash(curl *)"]},
            "hooks":{"PreToolUse":[]}
        })
        .to_string(),
    )
    .unwrap();
    let command = adapter().build_command("test", &opts()).unwrap();
    let args: Vec<_> = command.get_args().map(|arg| arg.to_str().unwrap()).collect();
    assert!(args.windows(2).any(|pair| pair == ["--setting-sources", ""]));
    let settings = args.windows(2).find(|pair| pair[0] == "--settings").unwrap()[1];
    let settings: serde_json::Value = serde_json::from_str(settings).unwrap();
    assert_eq!(settings.as_object().unwrap().len(), 2);
    assert_eq!(
        settings["permissions"]["deny"],
        serde_json::json!(["Bash(curl *)"])
    );
    assert!(
        settings["apiKeyHelper"]
            .as_str()
            .unwrap()
            .starts_with("/usr/bin/security ")
    );
    assert!(settings.get("env").is_none());
    assert!(settings.get("hooks").is_none());
    assert!(
        !ClaudeAgent
            .build_command("test", &opts())
            .unwrap()
            .get_args()
            .any(|arg| arg == "--setting-sources")
    );
}

#[test]
fn ordinary_route_usage_and_over_budget_results_do_not_consult_store_or_fail() {
    let (_home, _guard) = setup();
    let store = crate::store::Store::open_memory().unwrap();
    store.db().execute_batch("DROP TABLE tasks").unwrap();
    let mut info = CompletionInfo {
        tokens: None,
        status: TaskStatus::Done,
        model: None,
        cost_usd: None,
        exit_code: None,
    };
    for line in [
        r#"{"type":"assistant","message":{"usage":{"input_tokens":1000000}}}"#,
        r#"{"type":"result","is_error":false,"total_cost_usd":250}"#,
    ] {
        assert!(
            !record_stream_failure(&adapter(), line, &store, &TaskId("missing".into()), &mut info).unwrap()
        );
        assert_eq!(info.status, TaskStatus::Done);
    }
}
