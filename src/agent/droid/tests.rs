// Droid adapter tests: command flags and streamed event parsing.
// Exports no public API; validates DroidAgent behavior.
// Depends on crate::agent, crate::rate_limit, and crate::types.

use super::DroidAgent;
use crate::agent::{Agent, RunOpts};
use crate::paths;
use crate::rate_limit;
use crate::types::{AgentKind, EventKind, TaskId};

#[test]
fn build_command_uses_droid_exec() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = DroidAgent.build_command("test prompt", &opts).unwrap();
    assert_eq!(cmd.get_program().to_str().unwrap(), "droid");
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
    assert!(args.contains(&"exec".to_string()));
    assert!(args.contains(&"stream-json".to_string()));
    assert!(args.contains(&"--skip-permissions-unsafe".to_string()));
}

#[test]
fn build_command_read_only_uses_use_spec() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: true,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = DroidAgent.build_command("test", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
    // True read-only: must be --use-spec, NOT --auto low (which still allows file mods).
    assert!(args.contains(&"--use-spec".to_string()));
    assert!(!args.contains(&"--auto".to_string()));
    assert!(!args.contains(&"low".to_string()));
    // Read-only must not escalate to skip-permissions-unsafe.
    assert!(!args.contains(&"--skip-permissions-unsafe".to_string()));
}

#[test]
fn build_command_adds_context_files_via_append_system_prompt_file() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec!["docs/spec.md".to_string(), "notes/todo.txt".to_string()],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = DroidAgent.build_command("test", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
    // `-f` would replace the prompt source; we use --append-system-prompt-file instead.
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--append-system-prompt-file", "docs/spec.md"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--append-system-prompt-file", "notes/todo.txt"]));
    assert!(!args.iter().any(|a| a == "-f"));
}

#[test]
fn build_command_wires_session_id() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: Some("sess_abc123".to_string()),
        env: None,
        env_forward: None,
    };
    let cmd = DroidAgent.build_command("continue work", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
    assert!(args.windows(2).any(|pair| pair == ["-s", "sess_abc123"]));
}

#[test]
fn build_command_default_uses_skip_permissions_unsafe() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = DroidAgent.build_command("test", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
    assert!(args.contains(&"--skip-permissions-unsafe".to_string()));
    // --skip-permissions-unsafe cannot be combined with --auto.
    assert!(!args.contains(&"--auto".to_string()));
    assert!(!args.contains(&"--use-spec".to_string()));
}

#[test]
fn parses_tool_call_events_with_tool_name() {
    let agent = DroidAgent;
    let line = r#"{"type":"tool_call","id":"toolu_01","toolId":"Read","toolName":"Read","parameters":{"file_path":"src/main.rs"}}"#;
    let event = agent.parse_event(&TaskId("t-droid".to_string()), line).unwrap();
    assert_eq!(event.event_kind, EventKind::ToolCall);
    assert_eq!(event.detail, "Read");
}

// Regression: a single droid tool invocation emits BOTH `tool_call` and
// `tool_result`. Treating the result as a second ToolCall doubled the
// loop-detector input — 5 legit Reads → 10 events with detail "Read" and
// the LoopDetector tripped on legitimate exploration. tool_result and
// tool_use must not produce events.
#[test]
fn ignores_tool_result_and_tool_use_events_to_avoid_duplicates() {
    let agent = DroidAgent;
    let result_line = r#"{"type":"tool_result","toolName":"Read","output":"file contents"}"#;
    assert!(
        agent
            .parse_event(&TaskId("t-droid".to_string()), result_line)
            .is_none(),
        "tool_result must not emit a separate event"
    );
    let use_line = r#"{"type":"tool_use","toolName":"Read","input":{"file_path":"x"}}"#;
    assert!(
        agent
            .parse_event(&TaskId("t-droid".to_string()), use_line)
            .is_none(),
        "tool_use must not emit a separate event"
    );
}

#[test]
fn parses_mission_step_events_as_milestones() {
    let agent = DroidAgent;
    let line = r#"{"type":"mission_step","step":"1/3","description":"Scan the repo"}"#;
    let event = agent.parse_event(&TaskId("t-droid".to_string()), line).unwrap();
    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(event.detail, "1/3 Scan the repo");
}

#[test]
fn parses_session_forked_events_as_milestones() {
    let agent = DroidAgent;
    let line = r#"{"type":"session_forked","parent_id":"sess_parent","new_id":"sess_child"}"#;
    let event = agent.parse_event(&TaskId("t-droid".to_string()), line).unwrap();
    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(event.detail, "forked sess_child from sess_parent");
    assert_eq!(
        event
            .metadata
            .as_ref()
            .and_then(|value| value.get("agent_session_id"))
            .and_then(|value| value.as_str()),
        Some("sess_child")
    );
}

#[test]
fn marks_droid_rate_limits_from_status_and_error_type() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    rate_limit::clear_rate_limit(&AgentKind::Droid);
    let agent = DroidAgent;
    let line = r#"{"type":"error","status":429,"error_type":"rate_limit_exceeded"}"#;
    let event = agent.parse_event(&TaskId("t-droid".to_string()), line).unwrap();
    assert_eq!(event.event_kind, EventKind::Error);
    assert_eq!(event.detail, "rate_limit_exceeded");
    assert!(rate_limit::is_rate_limited(&AgentKind::Droid));
    rate_limit::clear_rate_limit(&AgentKind::Droid);
}

#[test]
fn build_command_with_dir_sets_cwd() {
    let opts = RunOpts {
        dir: Some("/tmp/test".to_string()),
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = DroidAgent.build_command("test", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
    assert!(args.contains(&"--cwd".to_string()));
    assert!(args.contains(&"/tmp/test".to_string()));
}
