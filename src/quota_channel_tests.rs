// Tests for the CLI/model channel split that quota detection rests on.
// Deps: super::{provider_attributable, Channel}, rate_limit_signatures::QUOTA_SIGNATURES.

use super::*;
use crate::rate_limit_signatures::QUOTA_SIGNATURES;

/// Everything admitted from the stream channel, both attribution strengths.
fn stream(output: &str, agent: AgentKind) -> String {
    provider_attributable(output, agent, Channel::CliStream).all()
}

/// Only what the CLI put inside a diagnostic envelope — the slice a bare status
/// token may be matched against.
fn stream_diagnostic(output: &str, agent: AgentKind) -> String {
    provider_attributable(output, agent, Channel::CliStream).cli_diagnostic
}

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
    let kept = stream(&the_incident_line(), AgentKind::Cursor);
    assert!(kept.is_empty(), "model-authored text must not survive: {kept}");
}

/// Stated over the whole table rather than the one needle that was quoted: no
/// signature we ship can be forged by an agent that merely read our source.
#[test]
fn no_signature_in_the_table_can_be_forged_from_model_text() {
    for signature in QUOTA_SIGNATURES {
        let quoted = format!(
            "In `src/rate_limit_signatures.rs` the needle is \"{}\", and the CLI prints \
             it verbatim when the pool is spent.",
            signature.needle
        );
        let kept = stream(&assistant_envelope(&quoted), signature.agent);
        assert!(
            kept.is_empty(),
            "needle {:?} survived the split for {:?}: {kept}",
            signature.needle,
            signature.agent
        );
    }
}

/// copilot's refusal — captured on t-03a68876 and t-80cf4b62 — is a
/// `session.error`, not an `error`. Matching only the bare word dropped the
/// whole event, so the monthly quota this module exists for was invisible to it.
#[test]
fn copilots_session_error_is_read_as_the_cli_speaking() {
    let event = r#"{"type":"session.error","data":{"message":"{\"error\":{\"message\":\"You have exceeded your monthly quota\",\"code\":\"quota_exceeded\"}}","requestFingerprint":{"messageCount":2}}}"#;
    let kept = stream_diagnostic(event, AgentKind::Copilot);
    assert!(
        kept.to_lowercase().contains("exceeded your monthly quota"),
        "copilot's real refusal envelope must survive: {kept}"
    );
}

#[test]
fn a_cli_error_envelope_survives_as_the_providers_own_sentence() {
    let event = r#"{"type":"error","errorCode":"quota_exceeded","message":"You have exceeded your monthly quota","requestFingerprint":{"messageCount":2}}"#;
    let kept = stream_diagnostic(event, AgentKind::Copilot);
    assert!(
        kept.lines().any(|line| line == "You have exceeded your monthly quota"),
        "provider sentence must survive whole and unescaped: {kept}"
    );
}

/// A tool's own failure is not the provider speaking, however the CLI flags it.
/// The audit of 2026-08-07 grepped this repo for cursor's needles; aid rendered
/// that call as an event and wrote a hold only a person could clear.
#[test]
fn a_failing_tool_call_is_not_a_provider_refusal() {
    let pattern = "you're out of usage|out of usage|ActionRequired";
    for envelope in [
        format!(
            r#"{{"type":"tool_call","subtype":"completed","tool_call":{{"grepToolCall":{{"args":{{"pattern":"{pattern}"}}}}}}}}"#
        ),
        format!(r#"{{"type":"tool.execution_complete","error":"grep failed: {pattern}"}}"#),
        format!(
            r#"{{"type":"user","tool_use_id":"toolu_1","is_error":true,"content":"{pattern}: no matches"}}"#
        ),
    ] {
        let kept = stream(&envelope, AgentKind::Cursor);
        assert!(
            kept.is_empty(),
            "a tool envelope is the model's request, not the provider's answer: {kept}"
        );
    }
}

#[test]
fn a_nested_opencode_billing_refusal_survives() {
    let event = r#"{"type":"error","error":{"name":"APIError","data":{"message":"Insufficient balance. Manage your billing here: https://x"}}}"#;
    let kept = stream(event, AgentKind::OpenCode);
    assert!(kept.contains("Insufficient balance"), "got {kept}");
}

/// grok is buffered, but it still wraps a refusal in an envelope the model
/// cannot author, so it is covered — while the `text` its model fills is not
/// read at all, even when that text quotes grok's own needle.
#[test]
fn grok_is_split_by_its_envelope_not_by_its_words() {
    let refusal = r#"{"type":"error","message":"API error (status 402 Payment Required): Grok Build usage balance exhausted"}"#;
    assert!(
        stream(refusal, AgentKind::Grok).contains("usage balance exhausted"),
        "grok's error envelope is provider testimony"
    );

    let report = r#"{"text":"The needle is \"usage balance exhausted\" per rate_limit_signatures.rs","stopReason":"end_turn"}"#;
    assert!(
        stream(report, AgentKind::Grok).is_empty(),
        "grok's answer text is the model talking"
    );
}

/// qwen writes its refusal into the slot its model otherwise fills, so that slot
/// stays readable for qwen alone — the one place a model's own words are still
/// read as provider testimony.
#[test]
fn the_terminal_result_slot_is_open_for_qwen_and_shut_for_everyone_else() {
    let envelope = r#"{"type":"result","text":"Quota exhausted: Your token-plan 5-hour quota has been exhausted."}"#;
    assert!(
        stream(envelope, AgentKind::Qwen).contains("Quota exhausted:"),
        "qwen's measured refusal channel must stay readable"
    );
    assert!(
        stream(envelope, AgentKind::Cursor).is_empty(),
        "no other agent has been measured refusing here"
    );
}

/// Admissible but not *strongly* attributed: it is the model's slot, so a bare
/// status token in it proves nothing.
#[test]
fn qwens_result_slot_is_never_strong_enough_for_a_bare_status_token() {
    let envelope = r#"{"type":"result","text":"The upstream RPC returned 429 twice during the run."}"#;
    assert!(
        stream_diagnostic(envelope, AgentKind::Qwen).is_empty(),
        "the model's own slot is never CLI-diagnostic evidence"
    );
}

/// A buffered plain-text CLI has no envelope to split on, so its whole stdout
/// stays admissible. Pinned so the limit is on the record.
#[test]
fn plain_text_lines_stay_admissible() {
    let refusal = "Individual quota reached. Please upgrade your subscription. Resets in 59m21s.";
    assert_eq!(stream(refusal, AgentKind::Antigravity).trim(), refusal);
    assert!(
        stream_diagnostic(refusal, AgentKind::Antigravity).is_empty(),
        "a line with no envelope around it is never CLI-diagnostic evidence"
    );
}

/// PTY-attached CLIs glue terminal escapes onto their JSON lines. Without
/// stripping them first the line fails to parse and falls through to the
/// plain-text branch, which would hand the model's own text straight back.
#[test]
fn terminal_escapes_do_not_smuggle_model_text_past_the_split() {
    let line = format!("\u{1b}[2K\u{1b}[0G{}", assistant_envelope("insufficient balance"));
    assert!(
        stream(&line, AgentKind::OpenCode).is_empty(),
        "an escaped assistant envelope is still an assistant envelope"
    );
}

#[test]
fn a_result_envelope_flagged_is_error_is_read_as_a_diagnostic() {
    let envelope = r#"{"type":"result","is_error":true,"result":"You have hit your usage limit. try again at Mar 21st, 2099 2:27 PM."}"#;
    assert!(
        stream_diagnostic(envelope, AgentKind::Codex).contains("hit your usage limit"),
        "an envelope the CLI marked failed is the CLI speaking"
    );
}

/// The backstop for what this module cannot see: an adapter marking on its own,
/// outside the split. Two rounds of this fix each closed one bypass and left
/// others running, so the question is asked of every adapter at once rather than
/// of the one that failed last.
#[test]
fn no_adapter_writes_a_marker_from_a_shape_the_model_can_author() {
    let mut cases: Vec<(AgentKind, String)> = Vec::new();
    // Each agent handed the needle it owns.
    for signature in QUOTA_SIGNATURES {
        for line in model_authored_shapes(signature.needle) {
            cases.push((signature.agent, line));
        }
    }
    // And every agent handed the shapes that need no needle: a bare status
    // token, and this crate's own source. `~/.aid/rate-limit-claude` was written
    // on 2026-08-07 by an agent quoting the signature table, and `Claude` owns
    // no needle, so nothing above would have reached it.
    for agent in adapters_under_test() {
        for text in [
            "429 Too Many Requests while calling the RPC provider",
            "402 Payment Required — the report is about a different service",
            "match_quota_signature reads rate_limit_signatures.rs; see rate_limit.rs",
            "We must respect the rate limit on the Base sequencer feed",
        ] {
            for line in model_authored_shapes(text) {
                cases.push((agent, line));
            }
        }
    }

    for (agent_kind, line) in cases {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(temp.path().join(".aid")).expect("aid dir");
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());

        let agent = crate::agent::get_agent(agent_kind);
        let _ = agent.parse_event(&crate::types::TaskId("t-forge".to_string()), &line);
        let _ = agent.parse_completion(&line);

        let written = markers_on_disk();
        assert!(
            written.is_empty(),
            "{agent_kind:?} wrote {written:?} from a line the model could author: {line}"
        );
    }
}

/// Every way a model's own words reach aid: rendered by the terminal, quoted in
/// an assistant envelope, or echoed back as a tool's argument or failure.
fn model_authored_shapes(needle: &str) -> Vec<String> {
    // An audit report is the worst case and the one that actually happened: it
    // quotes the needle *and* reads as an error line, because per-item verdicts
    // are written `FAILED`. Both halves matter — an adapter that gates marking
    // on "looks like an error" and then matches the needle admits exactly this.
    let report = format!("FAILED: | cursor | `{needle}` | yes | JSON `type:error` |");
    vec![
        format!("Error: the audit found \"{needle}\" in rate_limit_signatures.rs"),
        report.clone(),
        format!("completed: grep {needle}"),
        assistant_envelope(&report),
        assistant_envelope(&format!("The needle is \"{needle}\".")),
        format!(r#"{{"type":"message","content":"{report}"}}"#),
        format!(
            r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"toolu_1","is_error":true,"content":"grep: {needle}: no matches"}}]}}}}"#
        ),
        format!(
            r#"{{"type":"tool_call","subtype":"completed","tool_call":{{"grepToolCall":{{"args":{{"pattern":"{needle}"}}}}}}}}"#
        ),
        format!(r#"{{"type":"text","part":{{"text":"the needle is {needle}"}}}}"#),
    ]
}

/// Every kind `agent::get_agent` resolves — `AgentKind::ALL` minus `Custom`,
/// plus `Claude`, which `ALL` and `ALL_BUILTIN` both omit even though it has an
/// adapter. Iterating the adapters rather than those lists is what lets this
/// test see the agent whose marker went unlisted and unclearable.
fn adapters_under_test() -> Vec<AgentKind> {
    AgentKind::ALL
        .iter()
        .copied()
        .filter(|kind| *kind != AgentKind::Custom)
        .chain(std::iter::once(AgentKind::Claude))
        .collect()
}

fn markers_on_disk() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(crate::paths::aid_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("rate-limit-"))
        .collect()
}

/// stderr is kept whole, and that is the point: cursor's spent premium pool is
/// captured nowhere else.
#[test]
fn stderr_is_the_cursor_premium_channel_and_is_kept_whole() {
    let line = "ActionRequiredError: Increase limits for faster responses You're out of usage. \
                Switch to Auto, or ask your admin to increase your limit to continue.";
    let kept = provider_attributable(line, AgentKind::Cursor, Channel::CliStderr);
    assert!(kept.cli_diagnostic.contains("You're out of usage"));
    assert!(
        kept.unsplit.is_empty(),
        "stderr has no unsplit half — the whole channel is the CLI's"
    );
}
