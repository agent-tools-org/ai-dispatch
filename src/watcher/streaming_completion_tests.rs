// Streaming completion-status integration tests.
// Replays real ~/.aid/logs fixtures through watch_streaming with exit 0.
// Covers both directions: error envelope → Failed, success log → Done.

use std::process::Stdio;
use std::sync::Arc;

use crate::agent::claude::ClaudeAgent;
use crate::agent::commandcode::CommandCodeAgent;
use crate::agent::cursor::CursorAgent;
use crate::types::AgentKind;
use crate::agent::qwen::QwenAgent;
use crate::agent::Agent;
use crate::paths;
use crate::store::Store;
use crate::types::TaskStatus;

use super::streaming_tests::insert_running_task;
use super::watch_streaming;

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/streaming_completion/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

async fn watch_exit0(agent: &dyn Agent, output: &str, task_id: &str) -> TaskStatus {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = crate::types::TaskId(task_id.to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let mut child = tokio::process::Command::new("sh")
        .current_dir(temp.path())
        .arg("-c")
        .arg("cat; exit 0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use tokio::io::AsyncWriteExt;
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(output.as_bytes()).await.unwrap();
    }
    let info = watch_streaming(
        agent,
        &mut child,
        &task_id,
        &store,
        &log_path,
        None,
        crate::idle_timeout::DEFAULT_IDLE_TIMEOUT,
        None,
    )
    .await
    .unwrap();
    assert_eq!(info.exit_code, Some(0));
    info.status
}

#[tokio::test]
async fn exit0_qwen_api_error_fixture_records_failed() {
    let output = fixture("qwen-exit0-api-error.jsonl");
    assert!(
        output.contains("[API Error:"),
        "fixture must be real qwen API error log"
    );
    let status = watch_exit0(&QwenAgent, &output, "t-stream-qwen-err").await;
    assert_eq!(status, TaskStatus::Failed);
}

#[tokio::test]
async fn exit0_result_is_error_true_fixture_records_failed() {
    let output = fixture("stream-result-is-error-true.jsonl");
    assert!(
        output.contains("\"is_error\":true"),
        "fixture must carry real is_error:true result"
    );
    let status = watch_exit0(&CursorAgent, &output, "t-stream-cursor-err").await;
    assert_eq!(status, TaskStatus::Failed);
    let status = watch_exit0(&ClaudeAgent, &output, "t-stream-claude-err").await;
    assert_eq!(status, TaskStatus::Failed);
}

#[tokio::test]
async fn exit0_cursor_success_fixture_records_done() {
    let output = fixture("cursor-exit0-success.jsonl");
    assert!(
        output.contains("\"is_error\":false"),
        "fixture must be real cursor success result"
    );
    let status = watch_exit0(&CursorAgent, &output, "t-stream-cursor-ok").await;
    assert_eq!(status, TaskStatus::Done);
}

#[tokio::test]
async fn exit0_opencode_error_envelope_records_failed() {
    let output = fixture("opencode-error-envelope.jsonl");
    assert!(
        output.contains("\"type\":\"error\""),
        "fixture must carry real opencode nested error"
    );
    let status = watch_exit0(crate::agent::get_agent(AgentKind::OpenCode).as_ref(), &output, "t-stream-oc-err").await;
    assert_eq!(status, TaskStatus::Failed);
}

#[tokio::test]
async fn exit0_opencode_success_fixture_records_done() {
    let output = fixture("opencode-exit0-success.jsonl");
    assert!(
        !output.contains("\"type\":\"error\""),
        "success fixture must not include error events"
    );
    let status = watch_exit0(crate::agent::get_agent(AgentKind::OpenCode).as_ref(), &output, "t-stream-oc-ok").await;
    assert_eq!(status, TaskStatus::Done);
}

#[tokio::test]
async fn exit0_commandcode_failed_result_fixture_records_failed() {
    let output = fixture("commandcode-result-max-turns.jsonl");
    assert!(
        output.contains("\"subtype\":\"max_turns\""),
        "fixture must carry real Command Code result failure"
    );
    let status = watch_exit0(&CommandCodeAgent, &output, "t-stream-commandcode-max").await;
    assert_eq!(status, TaskStatus::Failed);
}

pub(super) async fn watch_usage(
    agent: &dyn Agent,
    output: &str,
    max_cost: Option<f64>,
) -> (crate::types::CompletionInfo, Vec<crate::types::TaskEvent>) {
    use std::os::unix::process::CommandExt;
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = crate::types::TaskId("t-opencode-usage".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let input_path = temp.path().join("input.jsonl");
    std::fs::write(&input_path, output).unwrap();
    let mut command = tokio::process::Command::new("sh");
    command.as_std_mut().process_group(0);
    let mut child = command
        .arg("-c")
        .arg("cat \"$1\"; if [ \"$2\" = keep-open ]; then exec cat; fi")
        .arg("replay")
        .arg(input_path)
        .arg(if max_cost.is_some() { "keep-open" } else { "exit" })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let info = watch_streaming(
        agent,
        &mut child,
        &task_id,
        &store,
        &temp.path().join("stream.log"),
        None,
        std::time::Duration::from_secs(5),
        max_cost,
    )
    .await
    .unwrap();
    (info, store.get_events(task_id.as_str()).unwrap())
}

#[tokio::test]
async fn opencode_multi_step_completion_records_sum() {
    let agent = crate::agent::get_agent(AgentKind::OpenCode);
    let (info, _) = watch_usage(agent.as_ref(), &fixture("opencode-multi-step.jsonl"), None).await;
    assert_eq!(info.status, TaskStatus::Done);
    assert_eq!(info.exit_code, Some(0));
    assert_eq!(info.tokens, Some(1_498_073));
    assert!((info.cost_usd.unwrap() - 0.10654006).abs() < 1e-12);
}

#[tokio::test]
async fn opencode_running_sum_enforces_cost_ceiling() {
    let agent = crate::agent::get_agent(AgentKind::OpenCode);
    let output = fixture("opencode-multi-step.jsonl");
    let (info, events) = watch_usage(agent.as_ref(), &output, Some(0.009)).await;
    assert_eq!(info.status, TaskStatus::Failed);
    assert_eq!(info.tokens, Some(29_465));
    assert!((info.cost_usd.unwrap() - 0.009527012).abs() < 1e-12);
    assert!(events.iter().any(|event| {
        event.event_kind == crate::types::EventKind::Error
            && event.detail.contains("exceeded ceiling")
    }));
}

#[test]
fn opencode_step_usage_accumulates_before_final_parsing() {
    let agent = crate::agent::get_agent(AgentKind::OpenCode);
    let task_id = crate::types::TaskId("t-opencode-steps".to_string());
    let mut info = crate::types::CompletionInfo {
        tokens: None,
        status: TaskStatus::Done,
        model: None,
        cost_usd: None,
        exit_code: None,
    };
    for (line, (tokens, cost)) in fixture("opencode-multi-step.jsonl")
        .lines()
        .zip([(14_346, 0.00851796), (29_465, 0.009527012)])
    {
        let event = agent.parse_event(&task_id, line).unwrap();
        super::apply_completion_event(&mut info, &event);
        assert_eq!(info.tokens, Some(tokens));
        assert!((info.cost_usd.unwrap() - cost).abs() < 1e-12);
    }
}

#[test]
fn opencode_fix_preserves_claude_and_codex_final_totals() {
    let claude = concat!(
        r#"{"type":"result","subtype":"success","result":"Hello!","total_cost_usd":0.14359275,"#,
        r#""session_id":"session-1","usage":{"input_tokens":4,"cache_creation_input_tokens":18821,"#,
        r#""cache_read_input_tokens":44733,"output_tokens":143},"#,
        r#""modelUsage":{"claude-opus-4-6[1m]":{"inputTokens":4}}}"#,
    );
    let codex = concat!(
        r#"{"type":"turn.completed","usage":{"input_tokens":232452,"#,
        r#""cached_input_tokens":211968,"output_tokens":5988}}"#,
    );
    let task_id = crate::types::TaskId("t-final-totals".to_string());
    for (kind, line, tokens, cost) in [
        (AgentKind::Claude, claude, 63_701, 0.14359275),
        (AgentKind::Codex, codex, 238_440, 1.0),
    ] {
        let agent = crate::agent::get_agent(kind);
        let mut info = crate::types::CompletionInfo {
            tokens: Some(1),
            status: TaskStatus::Done,
            model: None,
            cost_usd: Some(1.0),
            exit_code: None,
        };
        let event = agent.parse_event(&task_id, line).unwrap();
        for _ in 0..2 {
            super::apply_completion_event(&mut info, &event);
            assert_eq!(info.tokens, Some(tokens));
            assert_eq!(info.cost_usd, Some(cost));
        }
    }
}
