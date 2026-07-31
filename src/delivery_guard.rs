// Final-delivery evidence: Codex JSONL events plus a text check for plain-text agents.
// Exports DeliveryEvidence, DeliveryOutcome, and looks_like_delivered_report.
// Depends only on serde_json so validation stays pure and replayable.

use serde_json::Value;

pub(crate) const MIN_FINAL_MESSAGE_CHARS: usize = 200;

/// Fraction of narration lines above which captured text is treated as a tool log
/// rather than a deliverable.
const NARRATION_DOMINANCE: f32 = 0.6;

/// Openers plain-text agents use to announce the tool call they are about to make.
const NARRATION_OPENERS: &[&str] = &[
    "i will ",
    "i'll ",
    "i am going to ",
    "i'm going to ",
    "let me ",
    "now i ",
    "next, i ",
    "next i ",
];

/// Do the captured bytes look like a report the agent actually wrote, or like a
/// transcript of what it was about to do? Non-streaming agents (agy, gemini) print
/// both to the same stream, so a run that dies mid-investigation leaves behind a
/// plausible-looking file made entirely of pre-tool narration.
pub(crate) fn looks_like_delivered_report(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() < MIN_FINAL_MESSAGE_CHARS {
        return false;
    }
    if has_markdown_heading(trimmed) {
        return true;
    }
    !is_narration_dominated(trimmed)
}

fn has_markdown_heading(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with('#') && line.trim_start_matches('#').starts_with(' ')
    })
}

fn is_narration_dominated(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return true;
    }
    let narration = lines.iter().filter(|line| is_narration_line(line)).count();
    narration as f32 / lines.len() as f32 >= NARRATION_DOMINANCE
}

fn is_narration_line(line: &str) -> bool {
    let lowered = line.to_lowercase();
    let lowered = lowered
        .strip_prefix("[milestone]")
        .unwrap_or(&lowered)
        .trim_start();
    NARRATION_OPENERS
        .iter()
        .any(|opener| lowered.starts_with(opener))
}

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

    /// Verbatim shape of the t-f2f1e7c1 capture: agy died mid-audit and left only the
    /// lines announcing each tool call, which aid then persisted as the audit report.
    const NARRATION_CAPTURE: &str = "I will start by checking the list of permissions to see what actions and directories are available to us.\n\
I will run `pwd` to identify the current working directory of our workspace.\n\
I will run `git diff main..HEAD` to get the list of changes made in the branch.\n\
I will output the full git diff to a file in the scratch folder so we can read it without truncation.\n\
I will inspect the indexer snapshot file `crates/sr-indexer/src/snapshot.rs` to answer Q2.\n\
[MILESTONE] Analyzed Q2 regarding serialization determinism across indexer restarts.\n";

    #[test]
    fn rejects_pre_tool_narration_capture() {
        assert!(!super::looks_like_delivered_report(NARRATION_CAPTURE));
    }

    #[test]
    fn accepts_markdown_report() {
        let report = format!("## Findings\n\nQ1 PASS. {LONG_MESSAGE}");
        assert!(super::looks_like_delivered_report(&report));
    }

    #[test]
    fn accepts_prose_report_without_headings() {
        assert!(super::looks_like_delivered_report(LONG_MESSAGE));
    }

    #[test]
    fn rejects_text_below_minimum_length() {
        assert!(!super::looks_like_delivered_report("## Findings\nNo findings."));
    }

    #[test]
    fn narration_followed_by_a_real_report_still_counts_as_delivered() {
        let mixed = format!("{NARRATION_CAPTURE}\n## Findings\n\n{LONG_MESSAGE}");
        assert!(super::looks_like_delivered_report(&mixed));
    }
}
