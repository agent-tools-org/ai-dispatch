// Focused regression tests for structured output formatting.
// Exports: none; verifies Gemini and other streaming log shapes render cleanly.
// Deps: show_output hub, serde_json, tempfile.

use super::extract_messages_from_log;
use super::show_output_messages::UNRECOGNIZED_JSON_NOTICE_PREFIX;
use serde_json::json;
use tempfile::NamedTempFile;

fn write_jsonl(file: &NamedTempFile, events: &[serde_json::Value]) {
    let content = events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    std::fs::write(file.path(), content).unwrap();
}

#[test]
fn gemini_delta_messages_flush_across_tool_boundaries() {
    let file = NamedTempFile::new().unwrap();
    write_jsonl(
        &file,
        &[
            json!({"type":"message","role":"assistant","content":"Hello","delta":true}),
            json!({"type":"message","role":"assistant","content":" there","delta":true}),
            json!({"type":"tool_call","name":"Read","arguments":{"file":"src/main.rs"}}),
            json!({"type":"message","role":"assistant","content":"Done.","delta":true}),
            json!({"type":"result","status":"success"}),
        ],
    );

    let output = extract_messages_from_log(file.path(), true, None).unwrap();

    assert_eq!(
        output,
        "Hello there\n---\n[Read] {\"file\":\"src/main.rs\"}\n---\nDone."
    );
}

#[test]
fn gemini_top_level_text_events_keep_only_latest_revision() {
    let file = NamedTempFile::new().unwrap();
    write_jsonl(
        &file,
        &[
            json!({"type":"text","content":"Draft"}),
            json!({"type":"text","content":"Draft updated"}),
            json!({"type":"turn_complete"}),
        ],
    );

    let output = extract_messages_from_log(file.path(), true, None).unwrap();

    assert_eq!(output, "Draft updated");
}

#[test]
fn assistant_message_content_arrays_are_rendered_as_plain_text() {
    let file = NamedTempFile::new().unwrap();
    write_jsonl(
        &file,
        &[json!({
            "type":"message",
            "role":"assistant",
            "content":[
                {"type":"text","text":"Alpha"},
                {"type":"text","text":" beta"}
            ]
        })],
    );

    let output = extract_messages_from_log(file.path(), true, None).unwrap();

    assert_eq!(output, "Alpha beta");
}

#[test]
fn copilot_stream_dedupes_final_message_and_flushes_at_tool_boundaries() {
    let file = NamedTempFile::new().unwrap();
    write_jsonl(
        &file,
        &[
            json!({"type":"assistant.message_delta","data":{"deltaContent":"Inspecting "}}),
            json!({"type":"assistant.message_delta","data":{"deltaContent":"repo"}}),
            json!({"type":"tool.execution_start","data":{"toolName":"view","arguments":{"path":"Cargo.toml"}}}),
            json!({"type":"assistant.message_delta","data":{"deltaContent":"Done."}}),
            json!({"type":"assistant.message","data":{"content":"Done."}}),
            json!({"type":"result","exitCode":0}),
        ],
    );

    let output = extract_messages_from_log(file.path(), true, None).unwrap();

    assert_eq!(
        output,
        "Inspecting repo\n---\n[view] {\"path\":\"Cargo.toml\"}\n---\nDone."
    );
}

#[test]
fn claude_result_event_does_not_duplicate_assistant_text() {
    // Reduction of ~/.aid/logs/t-f1c8475a.jsonl: short assistant, then report as
    // assistant text, then result with the same report. finish()'s empty-messages
    // guard must drop pending; mutating it to always emit makes this go red.
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude-t-f1c8475a-reduction.jsonl");
    let output = extract_messages_from_log(&fixture, true, Some("claude")).unwrap();

    let first = "I'll start by reading the checklist file that defines the audit scope.";
    let report_marker = "[MILESTONE] Read commit f8a6ba0e in full";
    assert!(
        output.starts_with(first),
        "expected early assistant turn first, got: {}",
        &output[..output.len().min(120)]
    );
    assert!(
        output.contains(report_marker),
        "expected report body once, got: {}",
        &output[..output.len().min(200)]
    );
    let occurrences = output.matches(report_marker).count();
    assert_eq!(
        occurrences, 1,
        "report must appear exactly once (got {occurrences}); full output len={}",
        output.len()
    );
    let verdict = "# Verdict: **BLOCK**";
    assert_eq!(
        output.matches(verdict).count(),
        1,
        "distinctive verdict line must appear once; output len={}",
        output.len()
    );
}

#[test]
fn qwen_stream_json_result_event_renders_final_text() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/qwen-0.22.3-real-envelopes.jsonl");
    let output = extract_messages_from_log(&fixture, true, Some("qwen")).unwrap();
    assert!(
        output.starts_with("Unable to complete:"),
        "expected qwen result text, got: {output}"
    );
    assert!(
        !output.contains(UNRECOGNIZED_JSON_NOTICE_PREFIX),
        "must not emit unrecognized-json notice for qwen stream-json: {output}"
    );
}

#[test]
fn copilot_tool_failure_renders_error_line() {
    let file = NamedTempFile::new().unwrap();
    write_jsonl(
        &file,
        &[json!({
            "type":"tool.execution_complete",
            "data":{
                "toolName":"bash",
                "success":false,
                "error":"permission denied"
            }
        })],
    );

    let output = extract_messages_from_log(file.path(), true, None).unwrap();

    assert_eq!(output, "[bash] Error: permission denied");
}
