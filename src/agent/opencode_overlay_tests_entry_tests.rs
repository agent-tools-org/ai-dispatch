// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

#[test]
fn recorded_stream_event_parsing_parity() {
    let task_id = TaskId("t-stream-proof".into());
    let stream_lines = [
        (r#"{"type":"step_start"}"#, None),
        (
            r#"{"type":"message","content":"Analyzing files...","sessionID":"ses_proof_1"}"#,
            Some(EventKind::Reasoning),
        ),
        (
            r#"{"type":"tool_call","name":"bash","arguments":"cargo test"}"#,
            Some(EventKind::Test),
        ),
        (
            r#"{"type":"tool_call","name":"read","arguments":"src/lib.rs"}"#,
            Some(EventKind::ToolCall),
        ),
        (
            r#"{"type":"auto_compact","message":"compacted turns"}"#,
            Some(EventKind::Milestone),
        ),
        (
            r#"{"type":"step_finish","part":{"tokens":{"total":500,"input":400,"output":100},"cost":0.002}}"#,
            Some(EventKind::Completion),
        ),
        (
            r#"{"type":"completion","tokens":500}"#,
            Some(EventKind::Completion),
        ),
        ("Compiling test-crate v0.1.0", Some(EventKind::Build)),
    ];

    let oc = super::super::super::super::get_agent(AgentKind::OpenCode);
    for (line, expected_kind) in stream_lines {
        let event = oc.parse_event(&task_id, line);
        assert_eq!(
            event.as_ref().map(|e| e.event_kind),
            expected_kind,
            "line: {line}"
        );
        if let Some(ev) = event {
            assert_eq!(ev.task_id, task_id);
            assert!(!ev.detail.is_empty());
        }
    }
}
