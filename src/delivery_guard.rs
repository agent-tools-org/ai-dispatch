// Codex final-delivery evidence derived from ordered JSONL events.
// Exports DeliveryEvidence and DeliveryOutcome for watcher completion gating.
// Depends only on serde_json so validation stays pure and replayable.

use serde_json::Value;

pub(crate) const MIN_FINAL_MESSAGE_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeliveryOutcome {
    Delivered,
    MissingFinalDelivery {
        last_work_kind: Option<String>,
        last_message_chars: usize,
    },
}

#[derive(Debug, Default)]
pub(crate) struct DeliveryEvidence {
    sequence: u64,
    last_work_sequence: Option<u64>,
    last_work_kind: Option<String>,
    last_message_sequence: Option<u64>,
    last_message_chars: usize,
}

impl DeliveryEvidence {
    pub(crate) fn observe_codex_jsonl(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let Some(item) = value.get("item") else {
            return;
        };
        let Some(item_kind) = item.get("type").and_then(Value::as_str) else {
            return;
        };
        self.sequence += 1;
        if is_work_event(&value, item_kind) {
            self.last_work_sequence = Some(self.sequence);
            self.last_work_kind = Some(item_kind.to_string());
            return;
        }
        if is_completed_message(&value, item_kind) {
            let text = item
                .get("text")
                .or_else(|| item.get("content"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            self.last_message_sequence = Some(self.sequence);
            self.last_message_chars = text.trim().chars().count();
        }
    }

    pub(crate) fn validate(&self) -> DeliveryOutcome {
        let message_is_last = match (self.last_message_sequence, self.last_work_sequence) {
            (Some(message), Some(work)) => message > work,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if message_is_last && self.last_message_chars >= MIN_FINAL_MESSAGE_CHARS {
            return DeliveryOutcome::Delivered;
        }
        DeliveryOutcome::MissingFinalDelivery {
            last_work_kind: self.last_work_kind.clone(),
            last_message_chars: self.last_message_chars,
        }
    }
}

fn is_completed_message(value: &Value, item_kind: &str) -> bool {
    value.get("type").and_then(Value::as_str) == Some("item.completed")
        && item_kind == "agent_message"
}

fn is_work_event(value: &Value, item_kind: &str) -> bool {
    let event_kind = value.get("type").and_then(Value::as_str).unwrap_or_default();
    matches!(event_kind, "item.started" | "item.completed" | "item.updated")
        && !matches!(item_kind, "agent_message" | "reasoning")
}

#[cfg(test)]
mod tests {
    use super::{DeliveryEvidence, DeliveryOutcome};

    const LONG_MESSAGE: &str = "A final report with enough detail. A final report with enough detail. A final report with enough detail. A final report with enough detail. A final report with enough detail. A final report with enough detail.";

    fn observe(evidence: &mut DeliveryEvidence, event: serde_json::Value) {
        evidence.observe_codex_jsonl(&event.to_string());
    }

    #[test]
    fn accepts_substantive_message_after_work() {
        let mut evidence = DeliveryEvidence::default();
        observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
        observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":LONG_MESSAGE}}));
        assert_eq!(evidence.validate(), DeliveryOutcome::Delivered);
    }

    #[test]
    fn rejects_progress_message_followed_by_work() {
        let mut evidence = DeliveryEvidence::default();
        observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":LONG_MESSAGE}}));
        observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
        assert!(matches!(
            evidence.validate(),
            DeliveryOutcome::MissingFinalDelivery { .. }
        ));
    }

    #[test]
    fn rejects_short_trailing_fragment() {
        let mut evidence = DeliveryEvidence::default();
        observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
        observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":"done"}}));
        assert!(matches!(
            evidence.validate(),
            DeliveryOutcome::MissingFinalDelivery { last_message_chars: 4, .. }
        ));
    }

    #[test]
    fn accepts_tool_free_answer() {
        let mut evidence = DeliveryEvidence::default();
        observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":LONG_MESSAGE}}));
        assert_eq!(evidence.validate(), DeliveryOutcome::Delivered);
    }
}
