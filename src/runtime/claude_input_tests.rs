// Fixture-driven Claude transport and receipt regression tests.
// Uses an unpaid fake CLI with the published SDK replay schema.
use super::*;
use crate::agent::{Agent, RunOpts, claude::ClaudeAgent};

use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

fn task(store: &Store) -> TaskId {
    let task = TaskId("t-claude-input".into());
    store.db().execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at) VALUES (?1,'claude',?2,'running',?3)",
        rusqlite::params![task.as_str(), "initial prompt", chrono::Local::now().to_rfc3339()],
    ).unwrap();
    task
}

fn queue(store: &Store, task: &TaskId) -> i64 {
    store
        .insert_message(
            task.as_str(),
            MessageDirection::In,
            "guidance",
            MessageSource::Steer,
        )
        .unwrap()
        .id
}

fn command(mode: &str, output: &std::path::Path) -> tokio::process::Command {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut command = tokio::process::Command::new("python3");
    command
        .arg(root.join("claude-stream-input.py"))
        .arg(mode)
        .arg(root.join("claude-stream-input.jsonl"))
        .arg(output)
        .args([
            "-p",
            "initial prompt",
            "--output-format",
            "stream-json",
            "--verbose",
        ]);
    command
}

#[test]
fn launch_preserves_prompt_options_environment_and_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let context = dir.path().join("context.txt");
    std::fs::write(&context, "context contents").unwrap();
    let opts = RunOpts {
        dir: Some(dir.path().to_string_lossy().into()),
        model: Some("sonnet".into()),
        read_only: true,
        result_file: Some("result.md".into()),
        context_files: vec![context.to_string_lossy().into()],
        ..Default::default()
    };
    let mut command: tokio::process::Command =
        ClaudeAgent.build_command("inspect", &opts).unwrap().into();
    command
        .env("CLAUDE_TEST", "synthetic")
        .env_remove("ANTHROPIC_API_KEY");
    let prompt = prepare(&ClaudeAgent, &mut command).unwrap().unwrap();
    let args: Vec<_> = command
        .as_std()
        .get_args()
        .map(|arg| arg.to_str().unwrap())
        .collect();
    assert_eq!(args[0..3], ["-p", "--output-format", "stream-json"]);
    assert!(prompt.contains("context contents") && prompt.contains("EXCEPT the result file"));
    for pair in [
        ["--model", "sonnet"],
        ["--permission-mode", "dontAsk"],
        ["--input-format", "stream-json"],
    ] {
        assert!(args.windows(2).any(|args| args == pair));
    }
    assert!(args.contains(&"--replay-user-messages"));
    assert_eq!(command.as_std().get_current_dir(), Some(dir.path()));
    assert!(
        command
            .as_std()
            .get_envs()
            .any(|(key, value)| key == "ANTHROPIC_API_KEY" && value.is_none())
    );
    assert!(
        command
            .as_std()
            .get_envs()
            .any(|(key, value)| key == "CLAUDE_TEST" && value == Some("synthetic".as_ref()))
    );
    let help = include_str!("../../tests/fixtures/claude-stream-input-help.txt");
    assert!(help.contains("--input-format") && help.contains("--replay-user-messages"));
    let schema = include_str!("../../tests/fixtures/claude-replay-schema.txt");
    assert!(schema.contains("isReplay: true") && schema.contains("uuid: UUID"));
}

#[tokio::test]
async fn multiple_results_drain_and_stdin_closes_after_queued_steer_replay() {
    let _permit = crate::support::test_subprocess::acquire();
    let home = crate::support::test_env::TestHome::with_dirs();
    let store = Arc::new(Store::open_memory().unwrap());
    let task = task(&store);
    let id = queue(&store, &task);
    let output = home.path().join("input.json");
    let mut command = command("ack", &output);
    let prompt = prepare(&ClaudeAgent, &mut command).unwrap();
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    let log = home.path().join("agent.log");
    let info = tokio::time::timeout(
        Duration::from_secs(5),
        crate::runtime::watcher::watch_streaming_with_input(
            &ClaudeAgent,
            &mut child,
            &task,
            &store,
            &log,
            None,
            Duration::from_secs(60),
            None,
            prompt.as_deref(),
            None,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(info.status, TaskStatus::Done);
    assert!(receipt(&store, task.as_str(), id).unwrap());
    assert!(
        store.list_messages_for_task(task.as_str()).unwrap()[0]
            .delivered_at
            .is_some()
    );
    assert!(output.with_extension("json.closed").exists());
    let log = std::fs::read_to_string(log).unwrap();
    assert!(log.contains("first result") && log.contains("final result"));
    assert_eq!(
        store
            .get_events(task.as_str())
            .unwrap()
            .iter()
            .filter(|event| event.detail == "accepted by claude")
            .count(),
        1
    );
    assert!(!active(task.as_str()));
}

#[tokio::test]
async fn writes_wrong_replays_and_eof_leave_acceptance_unconfirmed_without_resend() {
    let _permit = crate::support::test_subprocess::acquire();
    let home = crate::support::test_env::TestHome::with_dirs();
    let store = Store::open_memory().unwrap();
    let task = task(&store);
    let id = queue(&store, &task);
    let output = home.path().join("input.json");
    let mut command = command("eof", &output);
    let prompt = prepare(&ClaudeAgent, &mut command).unwrap();
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    let mut input = Session::start(&mut child, &task, &store, prompt.as_deref())
        .await
        .unwrap()
        .unwrap();
    assert!(active(task.as_str()));
    assert!(
        crate::cmd::reply::ensure_interactive_input(
            &store.get_task(task.as_str()).unwrap().unwrap(),
            crate::cmd::reply::InputCommand::Steer,
        )
        .is_ok()
    );
    input.poll(&store, &task).await.unwrap();
    assert!(!receipt(&store, task.as_str(), id).unwrap());
    let mut rows = BufReader::new(child.stdout.take().unwrap()).lines();
    while let Some(line) = rows.next_line().await.unwrap() {
        input.observe(&store, &task, &line).unwrap();
    }
    child.wait().await.unwrap();
    let mut replay: Value = serde_json::from_str(
        include_str!("../../tests/fixtures/claude-stream-input.jsonl")
            .lines()
            .nth(2)
            .unwrap(),
    )
    .unwrap();
    input.observe(&store, &task, &replay.to_string()).unwrap();
    replay["uuid"] = json!(format!("00000000-0000-4000-8000-{:012x}", id));
    replay["isReplay"] = json!(false);
    input.observe(&store, &task, &replay.to_string()).unwrap();
    input.observe(&store, &task, "{malformed").unwrap();
    assert!(!receipt(&store, task.as_str(), id).unwrap());
    assert!(
        store.list_messages_for_task(task.as_str()).unwrap()[0]
            .delivered_at
            .is_none()
    );
    drop(input);
    assert!(!active(task.as_str()));
    assert!(
        crate::cmd::reply::ensure_interactive_input(
            &store.get_task(task.as_str()).unwrap().unwrap(),
            crate::cmd::reply::InputCommand::Steer,
        )
        .unwrap_err()
        .to_string()
        .contains("no steer message was queued")
    );
    let mut next = tokio::process::Command::new("sh");
    next.args(["-c", "cat > \"$1\"", "fake-claude"])
        .arg(&output)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut next = next.spawn().unwrap();
    let mut resumed = Session::start(&mut next, &task, &store, Some("initial prompt"))
        .await
        .unwrap()
        .unwrap();
    resumed.poll(&store, &task).await.unwrap();
    drop(resumed);
    next.wait().await.unwrap();
    assert_eq!(std::fs::read_to_string(output).unwrap().lines().count(), 1);
}
