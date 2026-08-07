// Tests for the CLI/model channel split that quota detection rests on.
// Deps: super::provider_attributable, rate_limit_signatures::QUOTA_SIGNATURES.

use super::*;
use crate::rate_limit_signatures::QUOTA_SIGNATURES;

/// The bytes that wrote `~/.aid/rate-limit-cursor` on 2026-08-07: a cursor audit
/// task reading this repo's own test fixture and quoting it back, streamed the
/// only way a model's words can reach aid — inside an assistant envelope.
fn the_incident_line() -> String {
    let quoted = "assert_rate_limit(r#\"{\"type\":\"error\",\"message\":\"quota exceeded for \
                  this workspace\"}\"#, true);\n====\ncommit 7881e2d";
    assistant_envelope(quoted)
}

fn assistant_envelope(text: &str) -> String {
    serde_json::json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": text}]}
    })
    .to_string()
}

#[test]
fn the_assistant_envelope_that_forged_a_cursor_hold_survives_nothing() {
    let kept = provider_attributable(&the_incident_line(), AgentKind::Cursor);
    assert!(kept.is_empty(), "model-authored text must not survive: {kept}");
}

/// The guarantee, stated over the whole table rather than over the one needle
/// that happened to be quoted: no signature we ship — including ones added
/// later — can be turned into a marker by an agent that merely read our source.
#[test]
fn no_signature_in_the_table_can_be_forged_from_model_text() {
    for signature in QUOTA_SIGNATURES {
        let quoted = format!(
            "In `src/rate_limit_signatures.rs` the needle is \"{}\", and the CLI prints \
             it verbatim when the pool is spent.",
            signature.needle
        );
        let kept = provider_attributable(&assistant_envelope(&quoted), signature.agent);
        assert!(
            kept.is_empty(),
            "needle {:?} survived the split for {:?}: {kept}",
            signature.needle,
            signature.agent
        );
    }
}

#[test]
fn a_cli_error_envelope_survives_as_the_providers_own_sentence() {
    let event = r#"{"type":"error","errorCode":"quota_exceeded","message":"You have exceeded your monthly quota","requestFingerprint":{"messageCount":2}}"#;
    let kept = provider_attributable(event, AgentKind::Copilot);
    assert!(
        kept.lines().any(|line| line == "You have exceeded your monthly quota"),
        "provider sentence must survive whole and unescaped: {kept}"
    );
}

#[test]
fn a_nested_opencode_billing_refusal_survives() {
    let event = r#"{"type":"error","error":{"name":"APIError","data":{"message":"Insufficient balance. Manage your billing here: https://x"}}}"#;
    let kept = provider_attributable(event, AgentKind::OpenCode);
    assert!(kept.contains("Insufficient balance"), "got {kept}");
}

/// grok is buffered, but it still wraps a refusal in an envelope the model
/// cannot author, so it is covered — while the `text` its model fills is not
/// read at all, even when that text quotes grok's own needle.
#[test]
fn grok_is_split_by_its_envelope_not_by_its_words() {
    let refusal = r#"{"type":"error","message":"API error (status 402 Payment Required): Grok Build usage balance exhausted"}"#;
    assert!(
        provider_attributable(refusal, AgentKind::Grok).contains("usage balance exhausted"),
        "grok's error envelope is provider testimony"
    );

    let report = r#"{"text":"The needle is \"usage balance exhausted\" per rate_limit_signatures.rs","stopReason":"end_turn"}"#;
    assert!(
        provider_attributable(report, AgentKind::Grok).is_empty(),
        "grok's answer text is the model talking"
    );
}

/// qwen writes its refusal into the slot its model otherwise fills, so that slot
/// stays readable for qwen alone. This test exists to make the exception visible:
/// it is the one place a model's own words are still read as provider testimony.
#[test]
fn the_terminal_result_slot_is_open_for_qwen_and_shut_for_everyone_else() {
    let envelope = r#"{"type":"result","text":"Quota exhausted: Your token-plan 5-hour quota has been exhausted."}"#;
    assert!(
        provider_attributable(envelope, AgentKind::Qwen).contains("Quota exhausted:"),
        "qwen's measured refusal channel must stay readable"
    );
    assert!(
        provider_attributable(envelope, AgentKind::Cursor).is_empty(),
        "no other agent has been measured refusing here"
    );
}

/// A buffered plain-text CLI has no envelope to split on, so its whole stdout
/// stays admissible. Pinned so the limit is a decision on the record rather than
/// something a later reader has to infer.
#[test]
fn plain_text_lines_stay_admissible() {
    let refusal = "Individual quota reached. Please upgrade your subscription. Resets in 59m21s.";
    assert_eq!(
        provider_attributable(refusal, AgentKind::Antigravity).trim(),
        refusal
    );
}

/// PTY-attached CLIs glue terminal escapes onto their JSON lines. Without
/// stripping them first the line fails to parse and falls through to the
/// plain-text branch, which would hand the model's own text straight back.
#[test]
fn terminal_escapes_do_not_smuggle_model_text_past_the_split() {
    let line = format!("\u{1b}[2K\u{1b}[0G{}", assistant_envelope("insufficient balance"));
    assert!(
        provider_attributable(&line, AgentKind::OpenCode).is_empty(),
        "an escaped assistant envelope is still an assistant envelope"
    );
}

#[test]
fn a_result_envelope_flagged_is_error_is_read_as_a_diagnostic() {
    let envelope = r#"{"type":"result","is_error":true,"result":"You have hit your usage limit. try again at Mar 21st, 2099 2:27 PM."}"#;
    assert!(
        provider_attributable(envelope, AgentKind::Codex).contains("hit your usage limit"),
        "an envelope the CLI marked failed is the CLI speaking"
    );
}
