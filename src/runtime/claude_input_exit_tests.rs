// Exited Claude children must settle through the normal watcher, including output and exit code.
// Deterministic closed-pipe tests cover initial input and an uncertain queued delivery.
use super::*;
use crate::agent::claude::ClaudeAgent;

use std::process::Stdio;

fn insert_task(store: &Store, task: &TaskId) {
    store.db().execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at) VALUES (?1,'claude','p','running',?2)",
        rusqlite::params![task.as_str(), chrono::Local::now().to_rfc3339()],
    ).unwrap();
}

async fn exited_child(code: i32) -> tokio::process::Child {
    let mut command = tokio::process::Command::new("sh");
    command.args([
        "-c",
        &format!(
            concat!(
                "printf '%s\\n' '{{\"type\":\"result\",\"subtype\":\"success\",",
                "\"result\":\"startup output\"}}'; exit {}"
            ),
            code,
        ),
    ]);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take();
    child.wait().await.unwrap();
    child.stdin = stdin;
    child
}

#[tokio::test]
async fn immediate_exit_without_reading_stdin_preserves_output_and_exit_status() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    for (code, status) in [(0, TaskStatus::Done), (7, TaskStatus::Failed)] {
        let mut child = exited_child(code).await;
        let task = TaskId(format!("t-exit-{code}"));
        insert_task(&store, &task);
        let log = home.path().join(format!("{code}.jsonl"));
        let info = crate::runtime::watcher::watch_streaming_with_input(
            &ClaudeAgent,
            &mut child,
            &task,
            &store,
            &log,
            None,
            Duration::from_secs(60),
            None,
            Some("initial prompt"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(info.exit_code, Some(code));
        assert_eq!(info.status, status);
        assert!(
            std::fs::read_to_string(log)
                .unwrap()
                .contains("startup output")
        );
        assert!(!active(task.as_str()));
    }
}

#[tokio::test]
async fn broken_pipe_while_sending_steer_stays_unconfirmed() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = crate::support::test_env::TestHome::with_dirs().store();
    let task = TaskId("t-exit-steer".into());
    insert_task(&store, &task);
    let id = store
        .insert_message(
            task.as_str(),
            MessageDirection::In,
            "guidance",
            MessageSource::Steer,
        )
        .unwrap()
        .id;
    let mut child = exited_child(7).await;
    let mut session = Session {
        stdin: child.stdin.take(),
        task: task.to_string(),
        pending: BTreeMap::new(),
        result: false,
    };
    session.poll(&store, &task).await.unwrap();
    session.poll(&store, &task).await.unwrap();
    assert!(session.stdin.is_none());
    assert!(!receipt(&store, task.as_str(), id).unwrap());
    assert!(
        store.list_messages_for_task(task.as_str()).unwrap()[0]
            .delivered_at
            .is_none()
    );
    let events = store.get_events(task.as_str()).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].detail,
        "Claude input delivery attempted, unconfirmed"
    );
}

#[test]
fn legacy_claude_refuses_reply_and_steer_before_queuing() {
    let (_home, store) = crate::support::test_env::TestHome::new().store();
    let task = TaskId("t-legacy-claude".into());
    insert_task(&store, &task);
    let task = store.get_task(task.as_str()).unwrap().unwrap();
    for (command, refusal) in [
        (
            crate::cmd::reply::InputCommand::Reply,
            "no reply message was queued",
        ),
        (
            crate::cmd::reply::InputCommand::Steer,
            "no steer message was queued",
        ),
    ] {
        let error = crate::cmd::reply::ensure_interactive_input(&task, command).unwrap_err();
        assert!(error.to_string().contains(refusal));
        assert!(
            store
                .list_messages_for_task(task.id.as_str())
                .unwrap()
                .is_empty()
        );
    }
}
