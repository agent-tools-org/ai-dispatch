// Tests for the OpenCode CLI adapter command builder and JSON event parsing.
// Covers session reuse, budget variants, context files, and milestone events.

use super::super::Agent;
use super::*;

#[test]
fn parses_step_finish_token_event() {
    let task_id = TaskId("t-step".to_string());
    let event = parse_json_event(
        &task_id,
        &serde_json::json!({
            "type": "step_finish",
            "part": {
                "tokens": {
                    "total": 16125,
                    "input": 14040,
                    "output": 2,
                    "reasoning": 0
                },
                "cost": 0.0
            }
        }),
        Local::now(),
    )
    .expect("step_finish events should parse");

    assert_eq!(event.event_kind, EventKind::Completion);
    assert_eq!(event.detail, "tokens: 14040 in + 2 out = 16125");
    assert_eq!(
        event.metadata,
        Some(serde_json::json!({
            "tokens": 16125,
            "input_tokens": 14040,
            "output_tokens": 2,
            "cost_usd": 0.0
        }))
    );
}

#[test]
fn parses_new_milestone_events() {
    for value in [
        serde_json::json!({"type": "auto_compact", "message": "compacted session"}),
        serde_json::json!({"type": "git_snapshot", "text": "snapshot saved"}),
    ] {
        let event = parse_json_event(&TaskId("t-ms".to_string()), &value, Local::now())
            .expect("milestone events should parse");
        assert_eq!(event.event_kind, EventKind::Milestone);
        assert!(!event.detail.is_empty());
    }
}

#[test]
fn build_command_includes_file_flags_for_context_files() {
    let opts = RunOpts {
        dir: Some("/project".to_string()),
        output: None,
        model: Some("test-model".to_string()),
        budget: false,
        read_only: false,
        context_files: vec!["src/types.rs".to_string(), "src/lib.rs".to_string()],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = OpenCodeAgent
        .build_command("test prompt", &opts)
        .expect("command should build");
    let args: Vec<String> = cmd
        .get_args()
        .map(|s: &std::ffi::OsStr| s.to_string_lossy().to_string())
        .collect();
    assert!(args.contains(&"--thinking".to_string()));
    assert!(args.contains(&"-f".to_string()));
    let f_indices: Vec<usize> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "-f")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(f_indices.len(), 2);
    assert_eq!(args[f_indices[0] + 1], "src/types.rs");
    assert_eq!(args[f_indices[1] + 1], "src/lib.rs");
    assert!(args.contains(&"test prompt".to_string()));
}

#[test]
fn extracts_session_id_from_json_event() {
    let task_id = TaskId("t-sess".to_string());
    let event = parse_json_event(
        &task_id,
        &serde_json::json!({
            "type": "message",
            "content": "test",
            "sessionID": "ses_abc123"
        }),
        Local::now(),
    )
    .expect("message events should parse");

    assert_eq!(
        event
            .metadata
            .as_ref()
            .and_then(|m| m.get("agent_session_id").and_then(|s| s.as_str())),
        Some("ses_abc123")
    );
}

#[test]
fn session_flags_appear_in_command() {
    let opts = RunOpts {
        dir: None,
        output: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: Some("ses_test123".to_string()),
        env: None,
        env_forward: None,
    };
    let cmd = OpenCodeAgent
        .build_command("test prompt", &opts)
        .expect("command should build");
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    assert!(args.contains(&"--session".to_string()));
    assert!(args.contains(&"ses_test123".to_string()));
    assert!(args.contains(&"--continue".to_string()));
    assert!(args.contains(&"--fork".to_string()));
}

#[test]
fn budget_mode_sets_minimal_variant() {
    let opts = RunOpts {
        dir: None,
        output: None,
        model: None,
        budget: true,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = OpenCodeAgent
        .build_command("test prompt", &opts)
        .expect("command should build");
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    assert!(args.windows(2).any(|pair| pair == ["--variant", "minimal"]));
}

#[test]
fn opencode_needs_pty() {
    assert!(OpenCodeAgent.needs_pty());
}

#[test]
fn codex_does_not_need_pty() {
    assert!(!super::super::codex::CodexAgent.needs_pty());
}

#[test]
fn no_session_flags_when_session_id_absent() {
    let opts = RunOpts {
        dir: None,
        output: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = OpenCodeAgent
        .build_command("test prompt", &opts)
        .expect("command should build");
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    assert!(!args.contains(&"--session".to_string()));
    assert!(!args.contains(&"--continue".to_string()));
    assert!(!args.contains(&"--fork".to_string()));
    assert!(!args.contains(&"--variant".to_string()));
}

#[test]
fn build_command_read_only_prepends_readonly_prefix() {
    let opts = RunOpts {
        dir: None,
        output: None,
        model: None,
        budget: false,
        read_only: true,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = OpenCodeAgent
        .build_command("analyze this code", &opts)
        .expect("command should build");
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    let last_arg = args.last().expect("should have prompt as last arg");
    assert!(last_arg.contains("READ-ONLY MODE"));
    assert!(last_arg.starts_with("IMPORTANT: READ-ONLY MODE"));
    assert!(last_arg.contains("analyze this code"));
}
