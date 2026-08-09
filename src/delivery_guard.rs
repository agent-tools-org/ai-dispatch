// Final-delivery evidence: Codex JSONL events plus a text check for plain-text agents.
// Exports DeliveryEvidence, DeliveryOutcome, and looks_like_delivered_report.
// Depends only on serde_json so validation stays pure and replayable.

use serde_json::Value;

pub(crate) const MIN_FINAL_MESSAGE_CHARS: usize = 200;

/// Openers plain-text agents use to announce the tool call they are about to make.
/// Deliberately first-person-singular: "we will monitor the logs" is a sentence a real
/// report writes, and treating it as narration would discard the report around it.
const NARRATION_OPENERS: &[&str] = &[
    "i will ",
    "i'll ",
    "i am going to ",
    "i'm going to ",
    "i am now ",
    "i'm now ",
    "let me ",
    "now i ",
];

/// Discourse markers that can precede an opener ("First, I'll ...").
const NARRATION_PREFIXES: &[&str] = &["first, ", "first ", "next, ", "next ", "then, ", "then ", "now, "];

/// Do the captured bytes look like a report the agent actually wrote, or like a
/// transcript of what it was about to do? Non-streaming agents (agy, gemini) print
/// both to the same stream, so a run that dies mid-investigation leaves behind a
/// plausible-looking file made entirely of pre-tool narration.
pub(crate) fn looks_like_delivered_report(text: &str) -> bool {
    let trimmed = text.trim();
    if crate::cmd::show::is_unrecognized_json_notice(trimmed) {
        return true;
    }
    // Only what follows the last announced tool call can be the deliverable - the same
    // ordering rule the Codex JSONL guard applies to messages versus work events.
    let substance = substance_after_narration(trimmed);
    // A heading is the shape the report instruction asks for, and a legitimate report can
    // be as short as "## Findings\nNo findings." - do not hold length against it.
    if has_markdown_heading(&substance) {
        return true;
    }
    // Without a heading, require prose: bulk alone would accept a directory listing or
    // other raw tool output that happened to trail the last announced tool call.
    substance.chars().count() >= MIN_FINAL_MESSAGE_CHARS && looks_like_prose(&substance)
}

/// Sentence terminators across the scripts agents actually write in - ASCII plus the
/// ideographic stops used by Chinese and Japanese and the Devanagari danda.
const SENTENCE_ENDINGS: &[char] =
    &['.', '!', '?', '。', '！', '？', '．', '｡', '।', '؟', '۔'];

const LIST_MARKERS: &[&str] = &["- ", "* ", "+ ", "• "];

/// Prose, or a structured list - as opposed to raw tool output that merely happens to be
/// long. A directory listing has neither sentence punctuation nor list markers.
fn looks_like_prose(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        line.ends_with(SENTENCE_ENDINGS)
            || LIST_MARKERS.iter().any(|marker| line.starts_with(marker))
            || starts_with_numbered_marker(line)
    })
}

fn starts_with_numbered_marker(line: &str) -> bool {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && line[digits..].starts_with(['.', ')']) && line[digits + 1..].starts_with(' ')
}

/// Text left after the last line announcing a tool call. Milestones are progress
/// markers, never deliverables, so they are not substance either.
fn substance_after_narration(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let start = lines
        .iter()
        .rposition(|line| is_narration_line(line))
        .map_or(0, |index| index + 1);
    lines[start..]
        .iter()
        .filter(|line| !line.is_empty() && !is_milestone_line(line))
        .copied()
        .collect::<Vec<&str>>()
        .join("\n")
}

fn is_milestone_line(line: &str) -> bool {
    line.to_lowercase().starts_with("[milestone]")
}

fn has_markdown_heading(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with('#') && line.trim_start_matches('#').starts_with(' ')
    })
}

fn is_narration_line(line: &str) -> bool {
    // An announcement of the next tool call is a short sentence. Anything report-length
    // is a report, even when it opens with "I will present the findings: ...".
    if line.chars().count() >= MIN_FINAL_MESSAGE_CHARS {
        return false;
    }
    let lowered = line.to_lowercase();
    let mut rest = lowered
        .strip_prefix("[milestone]")
        .unwrap_or(&lowered)
        .trim_start();
    if let Some(prefix) = NARRATION_PREFIXES
        .iter()
        .find(|prefix| rest.starts_with(**prefix))
    {
        rest = rest[prefix.len()..].trim_start();
    }
    NARRATION_OPENERS
        .iter()
        .any(|opener| rest.starts_with(opener))
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

    /// When the task produced a diff, that diff is the deliverable and a
    /// non-empty trailing agent message is enough. When there are no changes,
    /// the prose *is* the deliverable, so the length floor still applies —
    /// including audit/report runs dispatched without `--read-only`.
    pub(crate) fn validate(&self, produced_changes: bool) -> DeliveryOutcome {
        let message_is_last = match (self.last_message_sequence, self.last_work_sequence) {
            (Some(message), Some(work)) => message > work,
            (Some(_), None) => true,
            (None, _) => false,
        };
        let substantive = if produced_changes {
            self.last_message_chars > 0
        } else {
            self.last_message_chars >= MIN_FINAL_MESSAGE_CHARS
        };
        if message_is_last && substantive {
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
