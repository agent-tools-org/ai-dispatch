// Splits captured agent output into the part aid can attribute to the CLI.
// Exports: provider_attributable.
// Deps: serde_json, types::AgentKind, watcher::strip_terminal_escapes.

use serde_json::{Map, Value};

use crate::types::AgentKind;

/// How deep to walk an error envelope collecting its diagnostic strings.
/// opencode's is the deepest observed (`/error/data/message`); the bound only
/// exists so a pathological document cannot make this walk unbounded.
const MAX_DEPTH: u8 = 6;

/// The subset of a run's captured output that the CLI wrote, with everything the
/// model wrote removed.
///
/// A quota refusal is testimony about the provider, and only the CLI can give
/// it. The model can only put bytes *inside* a field the CLI opened: whatever it
/// emits arrives as an escaped string under `/message/content/0/text`, never as
/// a sibling `{"type":"error"}` line. That containment is the thing this module
/// rests on, and it is not a property of the bytes — no wording the model
/// chooses can promote its own text out of the field it is written into.
///
/// This exists because the previous rule was a property of the bytes. On
/// 2026-08-07 a cursor audit task read `src/agent/cursor_tests.rs:142` —
/// `assert_rate_limit(r#"{"type":"error","message":"quota exceeded for this
/// workspace"}"#, true);` — and quoted it back in its report. That report was
/// streamed as `{"type":"assistant",...}` chunks, `watcher::stream` handed the
/// chunk's text to `extract_rate_limit_from_stream_detail`, cursor's anchored
/// needle matched inside it, and aid wrote a real hold on a route that was
/// serving the whole time. The denylist meant to stop that
/// (`is_signature_source_citation`) looked for `needle:` and `QuotaSignature`,
/// which a test fixture does not contain — and could not have contained every
/// other way our own repo carries a needle: a commit message, a changelog entry,
/// an audit report quoting the refusal it reports on.
///
/// What survives here, line by line:
///
/// | line | kept |
/// |---|---|
/// | JSON error envelope (`type:"error"`, an `error` key, `errorCode`, `is_error:true`) | every string in it |
/// | JSON `result`/`turn_complete` envelope, for an agent whose refusal is known to land there | its top-level `text` |
/// | any other JSON object | nothing |
/// | not JSON | the line |
///
/// What this cannot catch, stated rather than papered over:
///
/// - A CLI that prints its refusal as bare stdout text alongside the model's
///   answer has no structural split at all, so on that channel the model's words
///   and the provider's are the same bytes. Of the built-ins only `agy` is in
///   that shape; `grok`, the other buffered agent, wraps its refusal in a
///   `{"type":"error"}` envelope and is covered.
/// - `qwen` reports an exhausted token plan as ordinary terminal result text
///   with `is_error:false` and exit 0 — the CLI writing into the slot its model
///   otherwise fills. Keeping that slot admissible for qwen keeps a measured
///   outage detectable and leaves qwen the one agent that can still quote its
///   own refusal into a marker. Losing that slot instead would re-open the
///   outage `record_quota_exhaustion` was written for.
/// - A provider whose refusal wording nobody has captured stays undetectable.
///   Unchanged, and still the honest answer.
pub(crate) fn provider_attributable(output: &str, agent: AgentKind) -> String {
    let mut kept = String::with_capacity(output.len() / 4);
    for raw in output.lines() {
        let cleaned = crate::watcher::strip_terminal_escapes(raw);
        let line = cleaned.as_ref().trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line).ok().and_then(|value| value.as_object().cloned()) {
            Some(object) => keep_envelope_strings(&object, agent, &mut kept),
            None => push_line(&mut kept, line),
        }
    }
    kept
}

fn keep_envelope_strings(object: &Map<String, Value>, agent: AgentKind, kept: &mut String) {
    if is_error_envelope(object) {
        collect_strings(&Value::Object(object.clone()), MAX_DEPTH, kept);
    } else if terminal_text_is_a_refusal_channel(agent) && is_terminal_result(object) {
        if let Some(text) = object.get("text").and_then(Value::as_str) {
            push_line(kept, text);
        }
    }
}

/// An envelope the CLI opened to report its own failure. The model never
/// produces one of these: its output is a value inside a field, not a line.
fn is_error_envelope(object: &Map<String, Value>) -> bool {
    object.get("type").and_then(Value::as_str) == Some("error")
        || object.contains_key("error")
        || object.contains_key("errorCode")
        || object.get("is_error").and_then(Value::as_bool) == Some(true)
}

fn is_terminal_result(object: &Map<String, Value>) -> bool {
    matches!(
        object.get("type").and_then(Value::as_str),
        Some("result" | "turn_complete")
    )
}

/// Agents measured to report a refusal in the same slot their model's answer
/// occupies. This is a record of captured channels, not of wording: adding an
/// entry says "this CLI writes its refusal here", and it is the only place where
/// a model's own words are still read as provider testimony.
fn terminal_text_is_a_refusal_channel(agent: AgentKind) -> bool {
    matches!(agent, AgentKind::Qwen)
}

/// Every string inside a diagnostic envelope, each on its own line, so the
/// signature match runs against the provider's sentence rather than the raw
/// JSON around it. `serde_json` has already undone the escaping, so a marker
/// written from this reads as the provider wrote it.
fn collect_strings(value: &Value, depth: u8, kept: &mut String) {
    if depth == 0 {
        return;
    }
    match value {
        Value::String(text) => push_line(kept, text),
        Value::Array(items) => {
            for item in items {
                collect_strings(item, depth - 1, kept);
            }
        }
        Value::Object(fields) => {
            for field in fields.values() {
                collect_strings(field, depth - 1, kept);
            }
        }
        _ => {}
    }
}

fn push_line(kept: &mut String, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    kept.push_str(text);
    kept.push('\n');
}

#[cfg(test)]
#[path = "quota_channel_tests.rs"]
mod tests;
