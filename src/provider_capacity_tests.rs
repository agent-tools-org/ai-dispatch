// Captured capacity refusals, channel isolation, expiration and clearing regressions.
// Deps: provider capacity helpers, route markers and isolated aid home.

use super::*;
const CAPACITY: &str = "Selected model is at capacity. Please try a different model.";

#[test]
fn provider_capacity_regression_rejects_raw_quotes_on_both_channels() {
    let reconnect = "Reconnecting... 5/5 (unexpected status 503 Service Unavailable)\n\
                     Error: stream disconnected before completion";
    for channel in [Channel::CliStream, Channel::CliStderr] {
        for raw in [CAPACITY, reconnect] {
            assert!(
                classify(raw, AgentKind::Codex, channel).is_none(),
                "{channel:?}"
            );
        }
    }
}

#[test]
fn provider_capacity_regression_rejects_other_envelopes_and_reconnect_quotes() {
    for kind in [
        "tool.error",
        "assistant",
        "item.completed",
        "turn.completed",
    ] {
        let refusal = serde_json::json!({"type": kind, "message": CAPACITY});
        let reconnect = serde_json::json!({"type": kind,
            "message": "Reconnecting... 5/5 (unexpected status 503 Service Unavailable)"});
        let final_error = serde_json::json!({"type": "turn.failed",
            "error": {"message": "stream disconnected before completion"}});
        for channel in [Channel::CliStream, Channel::CliStderr] {
            assert!(classify(&refusal.to_string(), AgentKind::Codex, channel).is_none());
            assert!(
                classify(
                    &format!("{reconnect}\n{final_error}"),
                    AgentKind::Codex,
                    channel
                )
                .is_none(),
                "{kind}"
            );
            assert!(
                classify(
                    &format!("{final_error}\n{reconnect}"),
                    AgentKind::Codex,
                    channel
                )
                .is_none(),
                "{kind}"
            );
        }
    }
}

#[test]
fn provider_capacity_classifies_exact_codex_error_without_quota_or_model_unavailable() {
    let raw = serde_json::json!({"type":"turn.failed", "error":{"message":CAPACITY}}).to_string();
    assert_eq!(
        classify(&raw, AgentKind::Codex, Channel::CliStream).as_deref(),
        Some(CAPACITY)
    );
    assert_eq!(
        classify(CAPACITY, AgentKind::Codex, Channel::CliStderr).as_deref(),
        Some(CAPACITY)
    );
    assert!(!rate_limit::is_rate_limit_error_for_agent(
        CAPACITY,
        &AgentKind::Codex
    ));
    assert!(crate::model_health::extract_model_unavailable_json_error(&raw).is_none());
    let item =
        serde_json::json!({"type":"item.completed", "item":{"type":"error", "message":CAPACITY}});
    assert!(classify(&item.to_string(), AgentKind::Codex, Channel::CliStream).is_some());
}

#[test]
fn provider_capacity_classifies_final_error_after_exhausted_503_reconnects() {
    let tail = "Reconnecting... 4/5 (unexpected status 503 Service Unavailable)\n\
                Reconnecting... 5/5 (unexpected status 503 Service Unavailable)\n\
                ERROR: stream disconnected before completion";
    assert!(classify(tail, AgentKind::Codex, Channel::CliStderr).is_some());
    assert!(classify(tail, AgentKind::Codex, Channel::CliStream).is_some());
    assert!(
        classify(
            &tail.replace("5/5", "3/5"),
            AgentKind::Codex,
            Channel::CliStderr
        )
        .is_none()
    );
    assert!(
        classify(
            &tail.replace("503", "401"),
            AgentKind::Codex,
            Channel::CliStderr
        )
        .is_none()
    );
    assert!(
        classify(
            tail.lines().next().expect("reconnect"),
            AgentKind::Codex,
            Channel::CliStderr
        )
        .is_none()
    );
}

#[test]
fn provider_capacity_ignores_assistant_and_tool_quotes() {
    for raw in [
        serde_json::json!({"type":"item.completed", "item":{"type":"agent_message", "text":CAPACITY}}),
        serde_json::json!({"type":"tool.error", "message":CAPACITY}),
    ] {
        assert!(classify(&raw.to_string(), AgentKind::Codex, Channel::CliStream).is_none());
    }
    assert!(
        classify(
            &format!("The report quotes: {CAPACITY}"),
            AgentKind::Codex,
            Channel::CliStream
        )
        .is_none()
    );
}

#[test]
fn provider_capacity_classifies_exhausted_json_reconnect_tail() {
    let stream = concat!(
        "{\"type\":\"error\",\"message\":\"Reconnecting... 5/5 (unexpected status 503 Service Unavailable)\"}\n",
        "{\"type\":\"turn.failed\",\"error\":{\"message\":\"stream disconnected before completion\"}}\n"
    );
    assert!(classify(stream, AgentKind::Codex, Channel::CliStream).is_some());
    let recovered = stream.replace(
        "\n{\"type\":\"turn.failed\"",
        concat!("\n{\"type\":\"turn.completed\",\"usage\":{}}\n{\"type\":\"turn.failed\""),
    );
    assert!(classify(&recovered, AgentKind::Codex, Channel::CliStream).is_none());
}

#[test]
fn provider_capacity_expiry_and_success_release_model_hold() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let agent = AgentKind::Codex;
    let model = Some("gpt-6.1-sol");
    rate_limit::mark_rate_limited_for_model(&agent, None, model, CAPACITY);
    assert!(notes(&agent, None)[0].contains("gpt-6.1-sol: provider capacity held until"));
    assert!(
        rate_limit::active_group_holds(&agent, None).is_empty(),
        "capacity is separate from quota pools"
    );
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![agent]);
    let declared = DeclaredTaskProfile {
        difficulty: Default::default(),
        budget: Default::default(),
        urgency: Default::default(),
        rigor: Default::default(),
    };
    let advice = crate::cmd::advise::build_report(None, "task", declared, None, None, 0, None);
    assert!(
        advice
            .notes
            .iter()
            .any(|note| note.contains("gpt-6.1-sol: provider capacity held until"))
    );
    let id = TaskId("t-capacity-label".into());
    paths::ensure_dirs().expect("dirs");
    std::fs::write(paths::stderr_path(id.as_str()), CAPACITY).expect("stderr");
    assert_eq!(
        terminal_label(&id, agent, TaskStatus::Failed),
        "provider capacity"
    );
    assert!(rate_limit::clear_rate_limit_for_model(&agent, None, model));
    assert!(notes(&agent, None).is_empty());
    let past = chrono::Local::now().naive_local() - chrono::Duration::minutes(1);
    std::fs::write(
        rate_limit::group_marker_path(&agent, None, &group(model)),
        format!(
            "recovery_at: {}\nmessage: {CAPACITY}\n",
            rate_limit::format_recovery(past)
        ),
    )
    .expect("expired");
    assert_ne!(
        availability_for_model(&agent, None, model).status,
        routes::RouteStatus::Held
    );
    assert!(notes(&agent, None).is_empty());
    rate_limit::mark_rate_limited_for_model(&agent, None, model, CAPACITY);
    assert!(rate_limit::clear_all_rate_limits_for_agent(&agent, None));
    assert!(notes(&agent, None).is_empty());
}
