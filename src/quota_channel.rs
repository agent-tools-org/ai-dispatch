// The named channels a quota refusal may be read from, and the split within each.
// Exports: Channel, Attributable, provider_attributable.
// Deps: serde_json, types::AgentKind, watcher::strip_terminal_escapes.

use serde_json::{Map, Value};

use crate::types::AgentKind;

/// How deep to walk an error envelope collecting its diagnostic strings.
/// opencode's is the deepest observed (`/error/data/message`); the bound only
/// exists so a pathological document cannot make this walk unbounded.
const MAX_DEPTH: u8 = 6;

/// Where a run's bytes came from. Quota detection reads these and nothing else.
///
/// **The rule is "these named channels, each with a reason" — not "only the
/// CLI's own error channel".** The stricter rule was considered and rejected on
/// evidence: it drops two refusals we have actually captured. `qwen` reports an
/// exhausted token plan on its ordinary result slot with `is_error:false` and
/// exit 0, and `cursor`'s spent premium pool arrives only as an
/// `ActionRequiredError` line on stderr — neither is an error envelope. Both are
/// outages this module exists to record.
///
/// What keeps an enumeration from decaying into the denylist it replaced: the
/// list lives here, every path that can write a marker goes through it, and each
/// entry names the capture that justifies it. Adding a channel means adding a
/// variant here, not a condition at a call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Channel {
    /// Raw lines off the CLI's stdout, exactly as captured, before any adapter
    /// has parsed them. Split further by envelope — see the table below.
    CliStream,
    /// The CLI's own stderr. Kept line for line: the model writes its answer to
    /// stdout and has no way to address stderr, so the split the stream channel
    /// needs does not arise here.
    ///
    /// The residual, stated rather than assumed away: a tool subprocess the CLI
    /// spawns can inherit this descriptor, so a line here is *the CLI's process
    /// group*, not strictly the CLI. That is bounded by what a match on this
    /// channel can do — a generic token (`429`, `402`) writes a five-minute
    /// `Hold::Transient` and nothing longer, so the worst case is one route
    /// stepped over for five minutes, never a hold that waits for a person.
    CliStderr,
}

/// The subset of captured output that the CLI wrote, with everything the model
/// wrote removed.
///
/// A quota refusal is testimony about the provider, and only the CLI can give
/// it. On `CliStream` the model can only put bytes *inside* a field the CLI
/// opened: whatever it emits arrives as an escaped string under
/// `/message/content/0/text`, never as a sibling `{"type":"error"}` line. That
/// containment is the thing this module rests on, and it is not a property of
/// the bytes — no wording the model chooses can promote its own text out of the
/// field it is written into.
///
/// This exists because the previous rule was a property of the bytes. On
/// 2026-08-07 a cursor audit task read `src/agent/cursor_tests.rs:142` —
/// `assert_rate_limit(r#"{"type":"error","message":"quota exceeded for this
/// workspace"}"#, true);` — and quoted it back in its report. The denylist meant
/// to stop that (`is_signature_source_citation`) looked for `needle:` and
/// `QuotaSignature`, which a test fixture does not contain — and could not have
/// contained every other way our own repo carries a needle.
///
/// What survives on `CliStream`, line by line:
///
/// | line | kept |
/// |---|---|
/// | JSON error envelope (`type` naming an error, an `error`/`errorCode` key, `is_error:true`) | every string in it |
/// | JSON tool envelope, even one reporting the tool failed | nothing |
/// | JSON `result`/`turn_complete` envelope, for an agent whose refusal is known to land there | its top-level `text` |
/// | any other JSON object | nothing |
/// | not JSON | the line |
///
/// What this cannot catch, stated rather than papered over:
///
/// - The last row is the model's own words whenever the transport has no
///   envelope to split on: a buffered plain-text CLI (`agy`), and every
///   PTY-attached CLI, whose captured buffer is the *rendered* answer rather
///   than the JSON behind it (`opencode` and its overlays). A report by one of
///   those agents that quotes its own provider's refusal wording verbatim can
///   still write a marker. The bound is that a non-JSON line is only ever
///   matched against that agent's own anchored signature, never a generic token
///   — see `stream_completion::prose_line_is_quota_refusal`.
/// - `qwen` reports an exhausted token plan as ordinary terminal result text
///   with `is_error:false` and exit 0 — the CLI writing into the slot its model
///   otherwise fills. Keeping that slot admissible for qwen keeps a measured
///   outage detectable and leaves qwen the one agent that can still quote its
///   own refusal into a marker. Losing that slot instead would re-open the
///   outage `record_quota_exhaustion` was written for.
/// - A provider whose refusal wording nobody has captured stays undetectable.
///   Unchanged, and still the honest answer.
pub(crate) fn provider_attributable(
    output: &str,
    agent: AgentKind,
    channel: Channel,
) -> Attributable {
    let mut kept = Attributable::default();
    for raw in output.lines() {
        let cleaned = crate::watcher::strip_terminal_escapes(raw);
        let line = cleaned.as_ref().trim();
        if line.is_empty() {
            continue;
        }
        if channel == Channel::CliStderr {
            push_line(&mut kept.cli_diagnostic, line);
            continue;
        }
        match serde_json::from_str::<Value>(line).ok().and_then(|value| value.as_object().cloned()) {
            Some(object) => keep_envelope_strings(&object, agent, &mut kept),
            None => push_line(&mut kept.unsplit, line),
        }
    }
    kept
}

/// Attributable text, kept apart by how strong the attribution is.
///
/// The two are not interchangeable, and collapsing them is what made the
/// non-JSON branch too wide. A string lifted out of an envelope the CLI opened
/// is the CLI's by construction, so a bare status token in it (`429`, `402`)
/// means what it says. A line with no envelope around it is only the CLI's if
/// the transport had no envelopes to begin with — and on a PTY that same line
/// may be the model's rendered answer, where `429` is as likely to be a task id.
/// So the second kind is matched against anchored per-agent signatures only.
#[derive(Debug, Default)]
pub(crate) struct Attributable {
    /// Strings the CLI put in a diagnostic envelope, or every line of a channel
    /// that is the CLI's by construction (`CliStderr`).
    pub(crate) cli_diagnostic: String,
    /// Lines with no envelope to split on.
    pub(crate) unsplit: String,
}

impl Attributable {
    /// Both kinds, for callers that apply the narrow signature-only rule to all
    /// of it anyway.
    pub(crate) fn all(&self) -> String {
        format!("{}{}", self.cli_diagnostic, self.unsplit)
    }
}

fn keep_envelope_strings(object: &Map<String, Value>, agent: AgentKind, kept: &mut Attributable) {
    if is_error_envelope(object) {
        collect_strings(&Value::Object(object.clone()), MAX_DEPTH, &mut kept.cli_diagnostic);
    } else if terminal_text_is_a_refusal_channel(agent) && is_terminal_result(object) {
        // The slot qwen's model otherwise fills, so it is read under the narrow
        // rule even though it arrived inside an envelope.
        if let Some(text) = object.get("text").and_then(Value::as_str) {
            push_line(&mut kept.unsplit, text);
        }
    }
}

/// An envelope the CLI opened to report its own failure. The model never
/// produces one of these: its output is a value inside a field, not a line.
fn is_error_envelope(object: &Map<String, Value>) -> bool {
    if describes_a_tool(object) {
        return false;
    }
    names_an_error_type(object)
        || object.contains_key("error")
        || object.contains_key("errorCode")
        || object.get("is_error").and_then(Value::as_bool) == Some(true)
}

/// The `type` values CLIs use for their own failures.
///
/// Matching only the bare word `error` was too narrow for real CLI events:
/// copilot's monthly-quota refusal — the one captured on t-03a68876 and
/// t-80cf4b62, and the reason this branch exists — arrives as
/// `{"type":"session.error","data":{"message":"…exceeded your monthly quota…"}}`
/// and was being dropped whole.
fn names_an_error_type(object: &Map<String, Value>) -> bool {
    object.get("type").and_then(Value::as_str).is_some_and(|kind| {
        kind == "error" || kind.ends_with(".error") || kind.ends_with("_error")
    })
}

/// A tool's own failure is not the provider speaking, however the CLI flags it.
///
/// A failed `grep`, a rejected edit, a missing file: each arrives as an error
/// *inside* a tool envelope, carrying text the model chose (the pattern it
/// searched for, the path it asked to write). On 2026-08-07 one of them — an
/// audit's own `grep "you're out of usage|out of usage|ActionRequired"` — became
/// a hold on cursor that no clock would ever release. This check runs before the
/// error test, not after it, so widening what counts as an error envelope can
/// never re-open that path.
fn describes_a_tool(object: &Map<String, Value>) -> bool {
    object.get("type").and_then(Value::as_str).is_some_and(|kind| kind.contains("tool"))
        || object.contains_key("tool_call")
        || object.contains_key("tool_use_id")
        || object.contains_key("toolCallId")
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
