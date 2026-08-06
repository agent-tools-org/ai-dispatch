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
/// Detection and marker writes always run when a refusal is present; only the
/// task verdict considers deliverables. Callers need both facts separately: a
/// run that delivered *and* hit a refusal keeps its Done status, but its marker
/// must survive — `watcher.rs` clears the marker on every Done, so collapsing
/// the two into one bool wiped the outage we had just recorded.
pub(crate) fn record_quota_exhaustion(
    output: &str,
    agent: crate::types::AgentKind,
    model: Option<&str>,
) -> QuotaOutcome {
    let tail = quota_scan_tail(output);
    if !agent_prose_quota_match(tail, agent) {
        return QuotaOutcome::None;
    }
    let detail = quota_line(tail, agent)
        .or_else(|| crate::rate_limit::extract_rate_limit_message(tail))
        .unwrap_or_else(|| tail.chars().take(200).collect());
    // An agent whose plan meters model families separately must only lose the
    // family that ran out. agy's gemini allowance and its claude allowance are
    // independent: marking the whole agent would strand a working one.
    match crate::agent::model_group::model_group(agent, model) {
        Some(group) => crate::rate_limit::mark_group_rate_limited(&agent, group, &detail),
        None => crate::rate_limit::mark_rate_limited(&agent, &detail),
    }
    if output_has_substantive_deliverable(output) {
        QuotaOutcome::RecordedDelivered
    } else {
        QuotaOutcome::RecordedFailed
    }
}

/// What a completed run's output said about its provider's quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuotaOutcome {
    /// No refusal present.
    None,
    /// A refusal was recorded, but the run still delivered. The provider is
    /// marked so routing avoids it; the task itself is not a failure.
    RecordedDelivered,
    /// A refusal was recorded and the run delivered nothing.
    RecordedFailed,
}

impl QuotaOutcome {
    /// A marker was written — the caller must not clear it as part of "success".
    pub(crate) fn recorded(self) -> bool {
        !matches!(self, Self::None)
    }

    pub(crate) fn should_fail(self) -> bool {
        matches!(self, Self::RecordedFailed)
    }
}

fn agent_prose_quota_match(output: &str, agent: crate::types::AgentKind) -> bool {
    output.lines().any(|line| prose_line_is_quota_refusal(line, agent))
}

/// A refusal in assistant-authored text is only recognised by its provider's own
/// anchored template. A generic token — `429`, `rate limit` — carries no
/// information here: on a standalone line it is as likely to be a task id or a
/// markdown heading the agent wrote as a provider status. Providers whose
/// refusal wording nobody has captured are undetectable on this channel, and
/// that is the honest answer rather than a guess.
fn prose_line_is_quota_refusal(line: &str, agent: crate::types::AgentKind) -> bool {
    if crate::rate_limit_signatures::is_signature_source_citation(line) {
        return false;
    }
    crate::rate_limit_signatures::match_quota_signature_for_agent(line, agent).is_some()
}

fn output_has_substantive_deliverable(output: &str) -> bool {
    crate::delivery_guard::looks_like_delivered_report(output)
}

/// The quota sentence itself, windowed around the phrase that reports it.
///
/// Taking the first 200 characters of the matching line is not enough: these
/// arrive as JSONL, whose leading 200 characters are `type`/`uuid`/`session_id`
/// metadata. Truncating there discarded the reset time along with the message,
/// so the marker showed raw JSON and fell back to a "~1h" guess for a five-hour
/// window.
pub(super) fn quota_line(output: &str, agent: crate::types::AgentKind) -> Option<String> {
    let line = output
        .lines()
        .find(|line| prose_line_is_quota_refusal(line, agent))?;
    let lower = line.to_lowercase();
    let anchor = lower
        .find("quota")
        .or_else(|| lower.find("usage limit"))
        .or_else(|| quota_signature_anchor(&lower, agent))
        .unwrap_or(0);
    let start = line
        .char_indices()
        .map(|(idx, _)| idx)
        .take_while(|idx| *idx <= anchor.saturating_sub(40))
        .last()
        .unwrap_or(0);
    Some(line[start..].chars().take(240).collect::<String>().trim().to_string())
}

fn quota_signature_anchor(lower: &str, agent: crate::types::AgentKind) -> Option<usize> {
    crate::rate_limit_signatures::QUOTA_SIGNATURES
        .iter()
        .find(|signature| signature.agent == agent && lower.contains(signature.needle))
        .and_then(|signature| lower.find(signature.needle))
}

/// The last few lines of output, where a terminal failure reports itself.
///
/// Scans the last 64 KB (65,536 bytes) of output, aligned to a line boundary.
/// 64 KB is large enough to ensure refusal lines are captured even if followed
/// by extensive post-refusal diagnostic dumps or tracebacks, while keeping
/// string scanning microsecond-fast. Aligning the window start to a line boundary
/// ensures anchored refusal signatures are never split mid-line.
fn quota_scan_tail(output: &str) -> &str {
    const TAIL_BYTES: usize = 65_536;
    if output.len() <= TAIL_BYTES {
        return output;
    }
    let mut start = output.len() - TAIL_BYTES;
    while start < output.len() && !output.is_char_boundary(start) {
        start += 1;
    }
    if let Some(pos) = output[start..].find('\n') {
        start += pos + 1;
    }
    &output[start..]
}

#[cfg(test)]
#[path = "stream_completion_tests.rs"]
mod tests;
