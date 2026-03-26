// Tests for the gemini CLI adapter covering both old (pre-0.35) and new (0.35+) formats.
// Validates event parsing, token extraction, model detection, and response extraction.

use super::*;

#[test]
fn test_extract_model() {
    let json = serde_json::json!({
        "modelVersion": "gemini-2.5-pro",
        "response": "test"
    });
    assert_eq!(extract_model(&json), Some("gemini-2.5-pro".to_string()));

    // Old format: models as array
    let json2 = serde_json::json!({
        "stats": {
            "models": [{"model": "gemini-1.5-flash"}]
        }
    });
    assert_eq!(extract_model(&json2), Some("gemini-1.5-flash".to_string()));

    // New format: models as object keyed by name
    let json_new = serde_json::json!({
        "stats": {
            "total_tokens": 100,
            "models": {
                "gemini-2.5-flash": {"total_tokens": 100}
            }
        }
    });
    assert_eq!(extract_model(&json_new), Some("gemini-2.5-flash".to_string()));

    let json3 = serde_json::json!({ "response": "test" });
    assert_eq!(extract_model(&json3), None);
}

#[test]
fn test_extract_tokens_new_format() {
    let json = serde_json::json!({
        "stats": {
            "total_tokens": 16953,
            "input_tokens": 16903,
            "output_tokens": 23,
            "models": {
                "gemini-2.5-flash": {
                    "total_tokens": 16953,
                    "input_tokens": 16903,
                    "output_tokens": 23
                }
            }
        }
    });
    assert_eq!(extract_tokens(&json), Some(16953));
}

#[test]
fn parses_text_event() {
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "text",
        "content": "Hello world"
    });
    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
    assert_eq!(event.task_id, task_id);
    assert_eq!(event.event_kind, EventKind::Reasoning);
    assert_eq!(event.detail, "Hello world");
}

#[test]
fn parses_turn_complete_with_tokens() {
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "turn_complete",
        "stats": {
            "models": [{
                "model": "gemini-2.5-pro",
                "tokens": {
                    "total": 1234,
                    "input": 500,
                    "output": 734
                }
            }]
        }
    });
    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
    assert_eq!(event.task_id, task_id);
    assert_eq!(event.event_kind, EventKind::Completion);
    assert!(event.detail.contains("1234"));
    let metadata = event.metadata.unwrap();
    assert_eq!(metadata["tokens"], 1234);
    assert_eq!(metadata["model"], "gemini-2.5-pro");
}

#[test]
fn parses_message_event_new_format() {
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "message",
        "timestamp": "2026-03-26T09:00:00.000Z",
        "role": "assistant",
        "content": "The file contains hello world.",
        "delta": true
    });
    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
    assert_eq!(event.event_kind, EventKind::Reasoning);
    assert_eq!(event.detail, "The file contains hello world.");
}

#[test]
fn skips_user_message_event() {
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "message",
        "role": "user",
        "content": "read test.txt"
    });
    assert!(parse_stream_event(&task_id, &json, Local::now()).is_none());
}

#[test]
fn parses_result_event_new_format() {
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "result",
        "timestamp": "2026-03-26T09:00:01.000Z",
        "status": "success",
        "stats": {
            "total_tokens": 16953,
            "input_tokens": 16903,
            "output_tokens": 23,
            "cached": 8295,
            "duration_ms": 2516,
            "tool_calls": 1,
            "models": {
                "gemini-2.5-flash": {
                    "total_tokens": 16953,
                    "input_tokens": 16903,
                    "output_tokens": 23
                }
            }
        }
    });
    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
    assert_eq!(event.event_kind, EventKind::Completion);
    assert!(event.detail.contains("16953"));
    let metadata = event.metadata.unwrap();
    assert_eq!(metadata["tokens"], 16953);
    assert_eq!(metadata["model"], "gemini-2.5-flash");
}

#[test]
fn extract_response_from_stream_json() {
    let output = r#"{"type":"text","content":"First line"}
{"type":"text","content":"Second line"}
{"type":"turn_complete"}"#;
    let result = extract_response(output);
    assert_eq!(result, Some("Second line".to_string()));
}

#[test]
fn extract_response_from_new_format_message_deltas() {
    let output = r#"{"type":"init","session_id":"abc","model":"gemini-2.5-flash"}
{"type":"message","role":"user","content":"say hello"}
{"type":"message","role":"assistant","content":"Hello","delta":true}
{"type":"message","role":"assistant","content":" there!","delta":true}
{"type":"result","status":"success","stats":{"total_tokens":100}}"#;
    let result = extract_response(output);
    assert_eq!(result, Some("Hello there!".to_string()));
}

#[test]
fn parses_tool_call_event() {
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "tool_call",
        "name": "Read",
        "arguments": "{\"file\": \"test.rs\"}"
    });
    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
    assert_eq!(event.event_kind, EventKind::ToolCall);
    assert!(event.detail.starts_with("Read("));
}

#[test]
fn parses_tool_call_event_from_function_call_object() {
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "tool_call",
        "functionCall": {
            "name": "Read",
            "args": { "file": "src/main.rs" }
        }
    });
    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
    assert_eq!(event.event_kind, EventKind::ToolCall);
    assert_eq!(event.detail, r#"Read({"file":"src/main.rs"})"#);
}

#[test]
fn parses_tool_call_event_from_alternate_name_fields() {
    let task_id = TaskId::generate();
    let function_call = serde_json::json!({
        "type": "tool_call",
        "function_call": "Glob",
        "parameters": { "pattern": "*.rs" }
    });
    let tool_name = serde_json::json!({
        "type": "tool_call",
        "toolName": "Read",
        "input": { "file": "src/lib.rs" }
    });
    let tool = serde_json::json!({
        "type": "tool_call",
        "tool": "Write",
        "arguments": "{\"file\":\"src/lib.rs\"}"
    });

    let function_call_event = parse_stream_event(&task_id, &function_call, Local::now()).unwrap();
    let tool_name_event = parse_stream_event(&task_id, &tool_name, Local::now()).unwrap();
    let tool_event = parse_stream_event(&task_id, &tool, Local::now()).unwrap();

    assert_eq!(function_call_event.detail, r#"Glob({"pattern":"*.rs"})"#);
    assert_eq!(tool_name_event.detail, r#"Read({"file":"src/lib.rs"})"#);
    assert_eq!(tool_event.detail, r#"Write({"file":"src/lib.rs"})"#);
}

#[test]
fn parses_gemini_cli_tool_use_event() {
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "tool_use",
        "tool_name": "grep_search",
        "tool_id": "grep_search_123_0",
        "parameters": { "pattern": "dispatch" }
    });
    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
    assert_eq!(event.event_kind, EventKind::ToolCall);
    assert_eq!(event.detail, r#"grep_search({"pattern":"dispatch"})"#);
}

#[test]
fn parses_tool_result_test_event() {
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "tool_result",
        "name": "run_tests",
        "output": "Tests passed successfully"
    });
    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
    assert_eq!(event.event_kind, EventKind::Test);
}
