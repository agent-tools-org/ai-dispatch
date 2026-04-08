// Focused regression tests for structured output formatting.
// Exports: none; verifies Gemini and other streaming log shapes render cleanly.
// Deps: show_output hub, serde_json, tempfile.

use super::extract_messages_from_log;
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

    let output = extract_messages_from_log(file.path(), true).unwrap();

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

    let output = extract_messages_from_log(file.path(), true).unwrap();

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

    let output = extract_messages_from_log(file.path(), true).unwrap();

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

    let output = extract_messages_from_log(file.path(), true).unwrap();

    assert_eq!(
        output,
        "Inspecting repo\n---\n[view] {\"path\":\"Cargo.toml\"}\n---\nDone."
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

    let output = extract_messages_from_log(file.path(), true).unwrap();

    assert_eq!(output, "[bash] Error: permission denied");
}
