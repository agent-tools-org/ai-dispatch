// Shared streaming completion status from real CLI JSONL envelopes.
// Used by adapters' parse_completion and by streaming finalize paths.
// Deps: serde_json, TaskStatus / CompletionInfo.

use serde_json::Value;

use crate::types::{CompletionInfo, TaskStatus};

/// Claude / Cursor / Qwen stream-json terminal `result` is a failure.
/// Keyed to real envelopes: `is_error:true` or non-`success` subtype.
pub(crate) fn result_envelope_failed(v: &Value) -> bool {
    if v.get("type").and_then(|t| t.as_str()) != Some("result") {
        return false;
    }
    if v.get("is_error").and_then(|b| b.as_bool()) == Some(true) {
        return true;
    }
    matches!(
        v.get("subtype").and_then(|s| s.as_str()),
        Some(sub) if sub != "success"
    )
}

/// Any JSONL line with top-level `type == "error"` (OpenCode / Gemini / etc.).
pub(crate) fn jsonl_has_error_type(output: &str) -> bool {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) == Some("error") {
            return true;
        }
    }
    false
}

/// Status from Claude/Cursor-style result envelopes plus any `type:error` line.
pub(crate) fn status_from_result_jsonl(output: &str) -> CompletionInfo {
    let mut failed = jsonl_has_error_type(output);
    if !failed {
        for line in output.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if result_envelope_failed(&v) {
                failed = true;
                break;
            }
        }
    }
    CompletionInfo {
        tokens: None,
        status: if failed {
            TaskStatus::Failed
        } else {
            TaskStatus::Done
        },
        model: None,
        cost_usd: None,
        exit_code: None,
    }
}

/// Status from `type == "error"` only — no plaintext heuristics.
pub(crate) fn status_from_error_type_jsonl(output: &str) -> CompletionInfo {
    CompletionInfo {
        tokens: None,
        status: if jsonl_has_error_type(output) {
            TaskStatus::Failed
        } else {
            TaskStatus::Done
        },
        model: None,
        cost_usd: None,
        exit_code: None,
    }
}

/// Merge parse_completion into streaming info: Failed wins; fill missing fields.
pub(crate) fn merge_parsed_completion(info: &mut CompletionInfo, parsed: CompletionInfo) {
    if parsed.status == TaskStatus::Failed {
        info.status = TaskStatus::Failed;
    }
    if info.tokens.is_none() {
        info.tokens = parsed.tokens;
    }
    if info.model.is_none() {
        info.model = parsed.model;
    }
    if info.cost_usd.is_none() {
        info.cost_usd = parsed.cost_usd;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_is_error_true_fails() {
        let v: Value = serde_json::from_str(
            r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#,
        )
        .unwrap();
        assert!(result_envelope_failed(&v));
    }

    #[test]
    fn result_success_is_error_false_ok() {
        let v: Value = serde_json::from_str(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#,
        )
        .unwrap();
        assert!(!result_envelope_failed(&v));
    }

    #[test]
    fn nested_opencode_error_type_fails() {
        let out = r#"{"type":"error","error":{"name":"UnknownError","data":{"message":"x"}}}"#;
        assert_eq!(
            status_from_result_jsonl(out).status,
            TaskStatus::Failed
        );
    }
}

/// Detect a provider's quota-exhaustion message anywhere in the captured output
/// and record it, regardless of exit code or envelope shape.
///
/// This runs on the SUCCESS path on purpose. qwen reports an exhausted plan as
/// ordinary result text with `is_error:false` and exit 0, so every check that
/// hangs off the failure path — including `mark_rate_limited`'s existing call
/// sites — never sees it. The observed result was an exhausted provider still
/// reported as healthy by `aid agent quota`, its refusal recorded as a success,
/// and the previous rate-limit marker cleared by that same "success".
///
/// Returns true when a signature matched, so the caller can fail the task.
pub(crate) fn record_quota_exhaustion(
    output: &str,
    agent: crate::types::AgentKind,
    model: Option<&str>,
) -> bool {
    // Only the tail is evidence. A quota refusal terminates the run, so its
    // message is at the end of the output; scanning the whole transcript instead
    // matched prose the agent had merely READ. On 2026-08-05 a cursor task that
    // opened docs/design/cli-adapter-audit.md — which contains the words
    // "exhausted quota" — was marked failed and cursor was locked out for twelve
    // hours, while the task had in fact succeeded.
    let tail = quota_scan_tail(output);
    if !crate::rate_limit::is_rate_limit_error(tail) {
        return false;
    }
    let output = tail;
    // Record the sentence that actually reports the quota, not the head of the
    // transcript: the marker is what `aid agent quota` shows a human, and the
    // first 200 bytes of a JSONL stream are the session init line.
    let detail = quota_line(output)
        .or_else(|| crate::rate_limit::extract_rate_limit_message(output))
        .unwrap_or_else(|| output.chars().take(200).collect());
    // An agent whose plan meters model families separately must only lose the
    // family that ran out. agy's gemini allowance and its claude allowance are
    // independent: marking the whole agent would strand a working one.
    match crate::agent::model_group::model_group(agent, model) {
        Some(group) => crate::rate_limit::mark_group_rate_limited(&agent, group, &detail),
        None => crate::rate_limit::mark_rate_limited(&agent, &detail),
    }
    true
}

/// The quota sentence itself, windowed around the phrase that reports it.
///
/// Taking the first 200 characters of the matching line is not enough: these
/// arrive as JSONL, whose leading 200 characters are `type`/`uuid`/`session_id`
/// metadata. Truncating there discarded the reset time along with the message,
/// so the marker showed raw JSON and fell back to a "~1h" guess for a five-hour
/// window.
fn quota_line(output: &str) -> Option<String> {
    let line = output.lines().find(|line| {
        let lower = line.to_lowercase();
        (lower.contains("quota") || lower.contains("usage limit"))
            && crate::rate_limit::is_rate_limit_error(line)
    })?;
    let lower = line.to_lowercase();
    let anchor = lower
        .find("quota")
        .or_else(|| lower.find("usage limit"))
        .unwrap_or(0);
    let start = line
        .char_indices()
        .map(|(idx, _)| idx)
        .take_while(|idx| *idx <= anchor.saturating_sub(40))
        .last()
        .unwrap_or(0);
    Some(line[start..].chars().take(240).collect::<String>().trim().to_string())
}

/// The last few lines of output, where a terminal failure reports itself.
fn quota_scan_tail(output: &str) -> &str {
    const TAIL_BYTES: usize = 4000;
    if output.len() <= TAIL_BYTES {
        return output;
    }
    let mut start = output.len() - TAIL_BYTES;
    while start < output.len() && !output.is_char_boundary(start) {
        start += 1;
    }
    &output[start..]
}
