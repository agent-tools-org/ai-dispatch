// Fixture-backed app-server event mapping and terminal outcome tests.
// Exercises the existing Codex event parser after protocol normalization.

use super::*;
use crate::agent::{Agent, codex::CodexAgent};
use crate::types::{EventKind, TaskId};

fn fixture() -> Vec<Value> {
    include_str!("../../../tests/fixtures/codex-app-server-steer.jsonl")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| row["dir"] == "in")
        .map(|row| row["msg"].clone())
        .collect()
}

#[test]
fn transcript_maps_messages_commands_usage_and_completion() {
    let mut normalizer = Events::default();
    let task = TaskId("t-fixture".into());
    let mut mapped = Vec::new();
    for message in fixture() {
        for event in normalizer.notification(&message) {
            if let Some(event) = CodexAgent.parse_event(&task, &event.to_string()) {
                mapped.push(event);
            }
        }
    }
    assert!(mapped.iter().any(|event| event.detail == "STEERED"));
    assert!(
        mapped
            .iter()
            .any(|event| event.event_kind == EventKind::ToolCall)
    );
    let completion = mapped
        .iter()
        .rev()
        .find(|event| event.event_kind == EventKind::Completion)
        .unwrap();
    let metadata = completion.metadata.as_ref().unwrap();
    assert_eq!(metadata["tokens"], 44350);
    assert_eq!(metadata["cached_input_tokens"], 35072);
    assert_eq!(metadata["output_tokens"], 60);
}

#[test]
fn failed_and_interrupted_turns_produce_failures_even_with_usage() {
    for status in ["failed", "interrupted"] {
        let mut events = Events::default();
        let values = events.notification(&json!({"method":"turn/completed","params":{
            "turn":{"id":"turn","status":status,"error":{"message":"specific failure"}}}}));
        assert_eq!(values.last().unwrap()["type"], "turn.failed");
        assert_eq!(values.last().unwrap()["message"], "specific failure");
    }
}

#[test]
fn file_changes_reasoning_and_command_failures_use_existing_events() {
    let task = TaskId("t-items".into());
    let mut events = Events::default();
    for (item, kind) in [
        (
            json!({"type":"fileChange","changes":[{"path":"src/main.rs","kind":{"type":"update"}}]}),
            EventKind::FileWrite,
        ),
        (
            json!({"type":"reasoning","summary":["a thought"],"content":[]}),
            EventKind::Reasoning,
        ),
        (
            json!({"type":"commandExecution","command":"false","exitCode":1}),
            EventKind::Error,
        ),
    ] {
        let value = events.notification(&json!({"method":"item/completed","params":{"item":item}}));
        let mapped = CodexAgent
            .parse_event(&task, &value[0].to_string())
            .unwrap();
        assert_eq!(mapped.event_kind, kind);
    }
}

#[test]
fn sparse_rate_limit_updates_do_not_imply_exhaustion_or_recovery() {
    assert!(quota_message(&json!({"primary":null,"secondary":null})).is_none());
    assert!(quota_message(&json!({"primary":{"usedPercent":42}})).is_none());
    let message =
        quota_message(&json!({"primary":{"usedPercent":100,"resetsAt":4070908800i64}})).unwrap();
    assert!(message.contains("try again at"));
    assert!(quota_message(&json!({"spendControlReached":true})).is_some());
    assert!(
        quota_message(&json!({"rateLimitReachedType":"workspace_owner_credits_depleted"}))
            .is_some()
    );
    let mut events = Events::default();
    let normalized = events.notification(&json!({"method":"account/rateLimits/updated",
        "params":{"rateLimits":{"primary":{"usedPercent":12,"windowDurationMins":300}}}}));
    assert_eq!(
        normalized[0]["rate_limits"]["primary"]["windowDurationMins"],
        300
    );
}

#[test]
fn resumed_thread_usage_excludes_previous_turns_and_duplicate_updates() {
    let mut events = Events::default();
    let message = json!({"method":"thread/tokenUsage/updated","params":{"tokenUsage":{
        "total":{"inputTokens":1100,"cachedInputTokens":600,"outputTokens":110},
        "last":{"inputTokens":100,"cachedInputTokens":50,"outputTokens":10}}}});
    let first = events.notification(&message);
    assert_eq!(first[0]["usage"]["input_tokens"], 100);
    let duplicate = events.notification(&message);
    assert_eq!(duplicate, first);
    let next = events.notification(
        &json!({"method":"thread/tokenUsage/updated","params":{"tokenUsage":{
        "total":{"inputTokens":1200,"cachedInputTokens":650,"outputTokens":120},
        "last":{"inputTokens":100,"cachedInputTokens":50,"outputTokens":10}}}}),
    );
    assert_eq!(next[0]["usage"]["input_tokens"], 200);
    assert_eq!(next[0]["usage"]["cached_input_tokens"], 100);
}

#[test]
fn usage_windows_with_paid_or_unlimited_credits_do_not_hold_codex() {
    for credits in [json!({"hasCredits":true}), json!({"unlimited":true})] {
        let snapshot = json!({"primary":{"usedPercent":100},"secondary":{"usedPercent":100},
            "rateLimitReachedType":"rate_limit_reached","credits":credits});
        assert!(quota_message(&snapshot).is_none());
    }
    assert!(
        quota_message(&json!({"primary":{"usedPercent":100},"credits":{"hasCredits":false}}))
            .is_some()
    );
    assert!(
        quota_message(&json!({"credits":{"hasCredits":true},"spendControlReached":true})).is_some()
    );
}

#[test]
fn tool_items_remain_in_logs_and_final_delivery_ordering() {
    use crate::task::delivery_guard::{DeliveryEvidence, DeliveryOutcome};
    for (kind, normalized) in [
        ("mcpToolCall", "mcp_tool_call"),
        ("webSearch", "web_search"),
        ("collabAgentToolCall", "collab_tool_call"),
        ("imageGeneration", "image_generation"),
        ("dynamicToolCall", "dynamic_tool_call"),
    ] {
        let mut events = Events::default();
        let items = events.notification(&json!({"method":"item/completed",
            "params":{"item":{"type":kind,"id":"tool","status":"completed"}}}));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["item"]["type"], normalized);
        let mut delivery = DeliveryEvidence::default();
        delivery.observe_codex_jsonl(
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"earlier"}}"#,
        );
        delivery.observe_codex_jsonl(&items[0].to_string());
        assert!(matches!(
            delivery.validate(),
            DeliveryOutcome::MissingFinalDelivery { .. }
        ));
    }
}

#[test]
fn exec_reasoning_stays_ignored_while_app_server_reasoning_is_mapped() {
    let task = TaskId("t-reasoning".into());
    let exec = r#"{"type":"item.completed","item":{"type":"reasoning","text":"summary"}}"#;
    assert!(CodexAgent.parse_event(&task, exec).is_none());
    let mut events = Events::default();
    let app = events.notification(&json!({"method":"item/completed",
        "params":{"item":{"type":"reasoning","summary":["summary"]}}}));
    assert_eq!(
        CodexAgent
            .parse_event(&task, &app[0].to_string())
            .unwrap()
            .event_kind,
        EventKind::Reasoning
    );
}

#[test]
fn final_items_are_recovered_once_from_the_completion_envelope() {
    let mut events = Events::default();
    let message = json!({"method":"turn/completed","params":{"turn":{"status":"completed",
        "error":null,"items":[{"id":"final","type":"agentMessage","text":"delivered"}]}}});
    let first = events.notification(&message);
    assert_eq!(first[0]["item"]["text"], "delivered");
    let repeated = events.notification(&message);
    assert!(
        repeated
            .iter()
            .all(|event| event["type"] != "item.completed")
    );
}
