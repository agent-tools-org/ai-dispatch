// Tests for the Qwen CLI adapter covering command flags and stream-json parsing.
// Validates the Gemini-compatible command shape and Qwen-specific result events.

use super::*;
use crate::agent::{Agent, RunOpts};

#[test]
fn build_command_uses_qwen_stream_json_flags() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: Some("coder-model".to_string()),
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };

    let cmd = QwenAgent.build_command("hello", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();

    assert_eq!(cmd.get_program().to_string_lossy(), "qwen");
    assert!(args.windows(2).any(|pair| pair == ["-o", "stream-json"]));
    assert!(args.iter().any(|arg| arg == "-y"));
    assert!(args.windows(2).any(|pair| pair == ["-m", "coder-model"]));
    assert!(args.windows(2).any(|pair| pair == ["-p", "hello"]));
}

#[test]
fn parses_qwen_assistant_event() {
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "assistant",
        "session_id": "session-123",
        "message": {
            "model": "coder-model",
            "content": [{ "type": "text", "text": "Planning the refactor." }]
        }
    });

    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();

    assert_eq!(event.event_kind, EventKind::Reasoning);
    assert_eq!(event.detail, "Planning the refactor.");
}

#[test]
fn parses_qwen_result_event_with_usage() {
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "result",
        "session_id": "session-123",
        "usage": {
            "input_tokens": 321,
            "output_tokens": 79,
            "cache_read_input_tokens": 40,
            "total_tokens": 440
        },
        "model": "coder-model"
    });

    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();

    assert_eq!(event.event_kind, EventKind::Completion);
    assert_eq!(event.detail, "completed with 440 tokens");
    let metadata = event.metadata.unwrap();
    assert_eq!(metadata["tokens"], 440);
    assert_eq!(metadata["input_tokens"], 321);
    assert_eq!(metadata["output_tokens"], 79);
    assert_eq!(metadata["cache_read_input_tokens"], 40);
    assert_eq!(metadata["model"], "coder-model");
    assert_eq!(metadata["agent_session_id"], "session-123");
}
