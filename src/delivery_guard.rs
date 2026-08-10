// Final-delivery evidence derived from structured agent protocol events.
// Exports DeliveryEvidence and DeliveryOutcome; depends only on serde_json.
// Content quality is deliberately outside this module's success contract.

use serde_json::Value;

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
    last_message: String,
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
            let trimmed = text.trim();
            self.last_message_sequence = Some(self.sequence);
            self.last_message = trimmed.to_string();
            self.last_message_chars = trimmed.chars().count();
        }
    }

    /// A structured, non-empty final message after the last work event is delivery.
    /// The original task contract may legitimately require a one-character answer;
    /// this layer therefore never grades length, prose style, or report quality.
    pub(crate) fn validate(&self) -> DeliveryOutcome {
        let message_is_last = match (self.last_message_sequence, self.last_work_sequence) {
            (Some(message), Some(work)) => message > work,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if message_is_last && !self.last_message.is_empty() {
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
        && !matches!(item_kind, "agent_message" | "reasoning" | "todo_list")
}

#[cfg(test)]
#[path = "delivery_guard_tests.rs"]
mod tests;
