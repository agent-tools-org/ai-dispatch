// Tests for the OpenCode CLI adapter command builder and JSON event parsing.
// Covers session reuse, budget variants, context files, and milestone events.

use super::super::{RunOpts, get_agent};
use super::*;
use crate::{routing::rate_limit};

#[test]
fn parses_new_milestone_events() {
    for value in [
        serde_json::json!({"type": "auto_compact", "message": "compacted session"}),
        serde_json::json!({"type": "git_snapshot", "text": "snapshot saved"}),
    ] {
        let event = parse_json_event(
            AgentKind::OpenCode,
            AgentKind::OpenCode,
            None,
            &TaskId("t-ms".to_string()),
            &value,
            Local::now(),
        )
        .expect("milestone events should parse");
        assert_eq!(event.event_kind, EventKind::Milestone);
        assert!(!event.detail.is_empty());
    }
}

#[test]
fn build_command_includes_file_flags_for_context_files() {
    let opts = RunOpts {
        dir: Some("/project".to_string()),
        model: Some("test-model".to_string()),
        context_files: vec!["src/types.rs".to_string(), "src/lib.rs".to_string()],
        ..Default::default()
    };
    let cmd = get_agent(AgentKind::OpenCode)
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
        AgentKind::OpenCode,
        AgentKind::OpenCode,
        None,
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
        session_id: Some("ses_test123".to_string()),
        ..Default::default()
    };
    let cmd = get_agent(AgentKind::OpenCode)
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
        budget: true,
        ..Default::default()
    };
    let cmd = get_agent(AgentKind::OpenCode)
        .build_command("test prompt", &opts)
        .expect("command should build");
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    assert!(args.windows(2).any(|pair| pair == ["--variant", "minimal"]));
}

#[test]
fn no_session_flags_when_session_id_absent() {
    let opts = RunOpts {
        ..Default::default()
    };
    let cmd = get_agent(AgentKind::OpenCode)
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
        result_file: Some("result.md".to_string()),
        read_only: true,
        ..Default::default()
    };
    let cmd = get_agent(AgentKind::OpenCode)
        .build_command("analyze this code", &opts)
        .expect("command should build");
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    let last_arg = args.last().expect("should have prompt as last arg");
    assert!(last_arg.contains("READ-ONLY MODE"));
    assert!(last_arg.starts_with("IMPORTANT: READ-ONLY MODE"));
    assert!(last_arg.contains("EXCEPT the result file specified in this prompt"));
    assert!(last_arg.contains("analyze this code"));
}

#[test]
fn build_command_read_only_without_result_file_keeps_strict_prefix() {
    let opts = RunOpts {
        read_only: true,
        ..Default::default()
    };
    let cmd = get_agent(AgentKind::OpenCode)
        .build_command("analyze this code", &opts)
        .expect("command should build");
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    let last_arg = args.last().expect("should have prompt as last arg");
    assert!(
        last_arg.contains("Do NOT modify, create, or delete any files. Only read and analyze.")
    );
}

/// Built-in OpenCode marks on the watcher. Live wording must still be a refusal.
#[test]
fn live_insufficient_balance_envelope_is_a_stream_refusal() {
    let _temp = crate::support::test_env::TestHome::new();
    rate_limit::clear_rate_limit(&AgentKind::OpenCode, None);
    let line = r#"{"type":"error","error":{"name":"APIError","data":{"message":"Insufficient balance. Manage your billing here: https://x"}}}"#;
    let _ = get_agent(AgentKind::OpenCode).parse_event(&TaskId("t-oc".to_string()), line);
    assert!(!rate_limit::is_rate_limited(&AgentKind::OpenCode, None));
    let refusal = rate_limit::refusal_on_channel(
        line,
        AgentKind::OpenCode,
        crate::routing::quota::channel::Channel::CliStream,
    );
    assert!(
        refusal
            .as_deref()
            .is_some_and(|t| t.to_lowercase().contains("insufficient balance")),
        "live wording must still be a stream refusal: {refusal:?}"
    );
}
