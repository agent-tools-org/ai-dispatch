// Tests structured final-delivery evidence without grading message content.
// Covers ordering, empty messages, exact short answers, and absent delivery.
// Deps: super::{DeliveryEvidence, DeliveryOutcome} and serde_json fixtures.

use super::{DeliveryEvidence, DeliveryOutcome};

fn observe(evidence: &mut DeliveryEvidence, event: serde_json::Value) {
    evidence.observe_codex_jsonl(&event.to_string());
}

fn message(text: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "item.completed",
        "item": { "type": "agent_message", "text": text }
    })
}

fn work() -> serde_json::Value {
    serde_json::json!({
        "type": "item.completed",
        "item": { "type": "command_execution" }
    })
}

#[test]
fn accepts_exact_short_answer_without_work() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, message("ok"));

    assert_eq!(evidence.validate(), DeliveryOutcome::Delivered);
}

#[test]
fn accepts_exact_short_answer_after_work() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, work());
    observe(&mut evidence, message("0"));

    assert_eq!(evidence.validate(), DeliveryOutcome::Delivered);
}

#[test]
fn rejects_message_followed_by_work() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, message("Investigation in progress"));
    observe(&mut evidence, work());

    assert!(matches!(
        evidence.validate(),
        DeliveryOutcome::MissingFinalDelivery { .. }
    ));
}

#[test]
fn rejects_empty_final_message() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, work());
    observe(&mut evidence, message("  \n  "));

    assert!(matches!(
        evidence.validate(),
        DeliveryOutcome::MissingFinalDelivery {
            last_message_chars: 0,
            ..
        }
    ));
}

#[test]
fn rejects_run_without_final_message() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, work());

    assert!(matches!(
        evidence.validate(),
        DeliveryOutcome::MissingFinalDelivery {
            last_message_chars: 0,
            ..
        }
    ));
}

#[test]
fn ignores_reasoning_and_todo_events_after_final_message() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, work());
    observe(&mut evidence, message("ok"));
    observe(
        &mut evidence,
        serde_json::json!({"type":"item.completed","item":{"type":"reasoning"}}),
    );
    observe(
        &mut evidence,
        serde_json::json!({"type":"item.updated","item":{"type":"todo_list"}}),
    );

    assert_eq!(evidence.validate(), DeliveryOutcome::Delivered);
}
