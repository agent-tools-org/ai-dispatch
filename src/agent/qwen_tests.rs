// Tests for the Qwen CLI adapter covering command flags and stream-json parsing.
// Validates the Gemini-compatible command shape and Qwen-specific result events.

use super::*;
use crate::agent::{Agent, RunOpts};

fn base_opts() -> RunOpts {
    RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    }
}

#[test]
fn build_command_uses_qwen_stream_json_flags() {
    let opts = RunOpts {
        model: Some("coder-model".to_string()),
        ..base_opts()
    };

    let cmd = QwenAgent.build_command("hello", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();

    assert_eq!(cmd.get_program().to_string_lossy(), "qwen");
    assert!(args.windows(2).any(|pair| pair == ["-o", "stream-json"]));
    assert!(!args.iter().any(|arg| arg == "-y"));
    assert!(!args.iter().any(|arg| arg == "--approval-mode"));
    assert!(!args.iter().any(|arg| arg == "--include-directories"));
    assert!(args.windows(2).any(|pair| pair == ["-m", "coder-model"]));
    assert!(args.windows(2).any(|pair| pair == ["-p", "hello"]));
}

#[test]
fn build_command_fails_on_read_only() {
    let opts = RunOpts {
        read_only: true,
        ..base_opts()
    };

    let res = QwenAgent.build_command("hello", &opts);
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err().to_string(),
        "qwen agent does not support read-only mode; omit --read-only, or use an agent that supports it (codex, claude, cursor, gemini, agy)"
    );
}

#[test]
fn build_command_sets_sandbox_flag() {
    let opts = RunOpts {
        sandbox: true,
        ..base_opts()
    };

    let cmd = QwenAgent.build_command("hello", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    assert!(args.iter().any(|arg| arg == "--sandbox"));
}

#[test]
fn build_command_sets_session_id_flag() {
    let opts = RunOpts {
        session_id: Some("session-123".to_string()),
        ..base_opts()
    };

    let cmd = QwenAgent.build_command("hello", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    assert!(args.windows(2).any(|pair| pair == ["-r", "session-123"]));
}

#[test]
fn print_qwen_command_line() {
    let opts = RunOpts {
        dir: Some("/path/to/project".to_string()),
        model: Some("qwen3.8-max".to_string()),
        sandbox: true,
        session_id: Some("session-abc-123".to_string()),
        ..base_opts()
    };
    let cmd = QwenAgent.build_command("fix the bug in main.rs", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    println!("QWEN_CMD: qwen {}", args.join(" "));
}

#[test]
fn build_command_does_not_set_gemini_trust_env() {
    let opts = base_opts();

    let cmd = QwenAgent.build_command("hello", &opts).unwrap();
    assert!(cmd
        .get_envs()
        .all(|(key, _)| key.to_string_lossy() != "GEMINI_CLI_TRUST_WORKSPACE"));
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

#[test]
fn caps_long_assistant_reasoning_and_keeps_full_in_metadata() {
    let task_id = TaskId::generate();
    let long_text = format!("Deep reasoning {}", "z".repeat(120));
    let json = serde_json::json!({
        "type": "assistant",
        "session_id": "session-123",
        "message": { "content": [{ "type": "text", "text": long_text }] }
    });

    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();

    assert!(event.detail.len() <= crate::agent::truncate::EVENT_DETAIL_MAX);
    assert!(event.detail.ends_with("..."));
    let metadata = event.metadata.expect("metadata with full text");
    assert_eq!(metadata["full"].as_str(), Some(long_text.as_str()));
    assert_eq!(metadata["agent_session_id"].as_str(), Some("session-123"));
}

#[test]
fn get_qwen_selected_model_reads_settings_json() {
    let temp_dir = tempfile::tempdir().unwrap();
    let qwen_dir = temp_dir.path().join(".qwen");
    std::fs::create_dir_all(&qwen_dir).unwrap();
    let settings_path = qwen_dir.join("settings.json");
    
    let settings_json = serde_json::json!({
        "model": {
            "name": "my-selected-qwen-model"
        },
        "modelProviders": {
            "openai": [
                {
                    "id": "my-selected-qwen-model"
                },
                {
                    "id": "other-qwen-model"
                }
            ]
        }
    });
    std::fs::write(&settings_path, serde_json::to_string(&settings_json).unwrap()).unwrap();

    let old_home = std::env::var_os("HOME");
    unsafe {
        std::env::set_var("HOME", temp_dir.path());
    }

    let selected = crate::model_catalog::get_qwen_selected_model();
    assert_eq!(selected, Some("my-selected-qwen-model".to_string()));

    // Test that build_command defaults to this selected model
    let opts = base_opts();
    let cmd = QwenAgent.build_command("hello", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    assert!(args.windows(2).any(|pair| pair == ["-m", "my-selected-qwen-model"]));

    // Restore HOME
    unsafe {
        if let Some(h) = old_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
    }
}

#[test]
fn parse_completion_detects_api_errors_and_returns_failed() {
    // Test case 1: Raw text containing [API Error: ...]
    let output1 = "some logs\n[API Error: 403 Access to model denied]\nmore logs";
    let info1 = QwenAgent.parse_completion(output1);
    assert_eq!(info1.status, TaskStatus::Failed);

    // Test case 2: JSON line with type error
    let output2 = serde_json::json!({
        "type": "error",
        "message": "Access to model denied"
    }).to_string();
    let info2 = QwenAgent.parse_completion(&output2);
    assert_eq!(info2.status, TaskStatus::Failed);

    // Test case 3: JSON line with type result and error payload
    let output3 = serde_json::json!({
        "type": "result",
        "status": "failed",
        "error": "Access to model denied"
    }).to_string();
    let info3 = QwenAgent.parse_completion(&output3);
    assert_eq!(info3.status, TaskStatus::Failed);

    // Test case 4: JSON line with type assistant containing API Error in text
    let output4 = serde_json::json!({
        "type": "assistant",
        "content": "Received [API Error: 403 Access to model denied]"
    }).to_string();
    let info4 = QwenAgent.parse_completion(&output4);
    assert_eq!(info4.status, TaskStatus::Failed);

    // Test case 5: Successful completion
    let output5 = serde_json::json!({
        "type": "result",
        "usage": {
            "total_tokens": 120
        },
        "model": "my-qwen-model"
    }).to_string();
    let info5 = QwenAgent.parse_completion(&output5);
    assert_eq!(info5.status, TaskStatus::Done);
    assert_eq!(info5.tokens, Some(120));
    assert_eq!(info5.model, Some("my-qwen-model".to_string()));
}
