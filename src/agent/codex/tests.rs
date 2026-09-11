// Codex adapter regression tests extracted from the inline test module.
// Deps: parent adapter helpers, tempfile, and command/event types.

use super::{
    parse_semver, resume_fallback_event,
    rollout_filename_matches, session_rollout_exists, CodexAgent,
    RESUME_FALLBACK_DETAIL,
};
use crate::agent::{Agent, CommandContext, RunOpts};
use crate::types::{EventKind, TaskId};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn semver_parsing() {
    assert_eq!(parse_semver("codex-cli 0.116.0"), Some((0, 116, 0)));
    assert_eq!(parse_semver("codex-cli 0.99.3"), Some((0, 99, 3)));
    assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
    assert_eq!(parse_semver("garbage"), None);
}

#[test]
fn version_comparison_for_model_flag() {
    assert!((0, 116, 0) >= (0, 116, 0));
    assert!((0, 117, 0) >= (0, 116, 0));
    assert!((1, 0, 0) >= (0, 116, 0));
    assert!((0, 115, 9) < (0, 116, 0));
    assert!((0, 0, 0) < (0, 116, 0));
}

#[test]
fn parses_agent_message_items() {
    let agent = CodexAgent;
    let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"Planning the next edit."}}"#;
    let event = agent
        .parse_event(&TaskId("t-msg".to_string()), line)
        .unwrap();
    assert_eq!(event.event_kind, EventKind::Reasoning);
    assert!(event.detail.contains("Planning"));
}

#[test]
fn parses_thread_started_session_id() {
    let agent = CodexAgent;
    let line = r#"{"type":"thread.started","thread_id":"019d1efa-5aa6-7132-bdfa-71fb97e12438"}"#;
    let event = agent
        .parse_event(&TaskId("t-thread".to_string()), line)
        .unwrap();
    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(
        event
            .metadata
            .unwrap()
            .get("agent_session_id")
            .and_then(|v| v.as_str()),
        Some("019d1efa-5aa6-7132-bdfa-71fb97e12438")
    );
}

#[test]
fn builds_resume_fallback_milestone() {
    let event = resume_fallback_event(&TaskId("t-resume-fallback".to_string()));
    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(event.detail, RESUME_FALLBACK_DETAIL);
}

#[test]
fn finds_only_full_session_id_rollout_matches_in_nested_sessions_dir() {
    let temp = tempdir().unwrap();
    let sessions = temp.path().join("sessions/2026/08/09");
    fs::create_dir_all(&sessions).unwrap();
    let session_id = "019e3e49-6b83-7563-a3d8-b51a3a716dd1";
    fs::write(
        sessions.join(format!("rollout-2026-08-09T17-20-31-{session_id}.jsonl")),
        "{}",
    )
    .unwrap();
    fs::write(
        sessions.join("rollout-2026-08-09T17-20-31-extra-019e3e49-6b83-7563-a3d8-b51a3a716dd1.jsonl"),
        "{}",
    )
    .unwrap();
    fs::write(sessions.join("rollout-2026-08-09T17-20-31-session-123.jsonl"), "{}").unwrap();

    assert!(session_rollout_exists(
        temp.path().join("sessions").as_path(),
        session_id
    ));
    assert!(!session_rollout_exists(
        temp.path().join("sessions").as_path(),
        "123"
    ));
    assert!(session_rollout_exists(
        temp.path().join("sessions").as_path(),
        "session-123"
    ));
}

#[test]
fn sandboxed_resume_skips_host_rollout_precheck() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: true,
        context_files: vec![],
        session_id: Some("not-a-uuid-session".to_string()),
        env: None,
        env_forward: None,
    };
    let cmd = CodexAgent
        .build_command_with_context(
            "continue",
            &opts,
            CommandContext {
                durable_codex_home: false,
                cargo_target_dir: None,
                temp_dir: None,
            },
        )
        .unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    assert_eq!(&args[..3], ["exec", "resume", "--json"]);
    assert!(args.contains(&"not-a-uuid-session".to_string()));
}

#[test]
fn rollout_match_requires_exact_timestamp_boundary_and_session_id() {
    let path = Path::new(
        "rollout-2026-08-09T17-20-31-extra-019e3e49-6b83-7563-a3d8-b51a3a716dd1.jsonl",
    );
    assert!(!rollout_filename_matches(
        path,
        "019e3e49-6b83-7563-a3d8-b51a3a716dd1"
    ));
    assert!(!rollout_filename_matches(
        Path::new("rollout-2026-08-09T17-20-31-long-123.jsonl"),
        "123"
    ));
    assert!(rollout_filename_matches(
        Path::new("rollout-2026-08-09T17-20-31-session-123.jsonl"),
        "session-123"
    ));
    assert!(!rollout_filename_matches(
        Path::new("rollout-not-a-timestamp-019e3e49-6b83-7563-a3d8-b51a3a716dd1.jsonl"),
        "019e3e49-6b83-7563-a3d8-b51a3a716dd1"
    ));
}

#[test]
fn parses_file_change_events() {
    let agent = CodexAgent;
    let line = r#"{"type":"item.completed","item":{"id":"item_5","type":"file_change","changes":[{"path":"/tmp/test.txt","kind":"update"}],"status":"completed"}}"#;
    let event = agent
        .parse_event(&TaskId("t-file".to_string()), line)
        .unwrap();
    assert_eq!(event.event_kind, EventKind::FileWrite);
    assert!(event.detail.contains("test.txt"));
}

#[test]
fn parses_item_error_events() {
    let agent = CodexAgent;
    let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"error","message":"Model metadata for `o3` not found."}}"#;
    let event = agent
        .parse_event(&TaskId("t-err".to_string()), line)
        .unwrap();
    assert_eq!(event.event_kind, EventKind::Error);
    assert!(event.detail.contains("Model metadata"));
}

#[test]
fn parses_turn_completed_usage_metadata() {
    let agent = CodexAgent;
    let line = r#"{"type":"turn.completed","usage":{"input_tokens":232452,"cached_input_tokens":211968,"output_tokens":5988}}"#;
    let event = agent
        .parse_event(&TaskId("t-usage".to_string()), line)
        .unwrap();
    assert_eq!(event.event_kind, EventKind::Completion);
    assert_eq!(
        event
            .metadata
            .unwrap()
            .get("tokens")
            .and_then(|v| v.as_i64()),
        Some(238440)
    );
}
