// Round-two regressions for OpenCode PTY auth and atomic wrap-up delivery.
// Exercises real PTY capture, monitor input forwarding, and SQLite commit failures.

use crate::agent::get_agent;
use crate::paths::{self, AidHomeGuard};
use crate::pty_bridge::PtyBridge;
use crate::pty_watch::MonitorState;
use crate::store::Store;
use crate::types::{AgentKind, EventKind, TaskStatus};
use std::sync::Arc;

const ERROR: &str = concat!(
    r#"{"type":"error","error":{"name":"APIError","data":{"statusCode":401,"#,
    r#""message":"Unauthorized"}}}"#,
);
const ASSISTANT: &str = r#"{"type":"text","part":{"text":"Continuing normally"}}"#;

fn captured_auth(output: &str, exit: i32) -> bool {
    let _permit = crate::test_subprocess::acquire();
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("directories");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut task = crate::pty_watch::tests::pty_task("t-pty-auth", TaskStatus::Running);
    task.agent = AgentKind::OpenCode;
    store.insert_task(&task).expect("task");
    let agent = get_agent(AgentKind::OpenCode);
    let mut command = std::process::Command::new("/bin/sh");
    command.args([
        "-c",
        "printf '%s\n' \"$1\" >&2; exit \"$2\"",
        "fixture",
        output,
        &exit.to_string(),
    ]);
    crate::pty_runner::run_agent_process(
        agent.as_ref(),
        &command,
        &task.id,
        &store,
        &paths::log_path(task.id.as_str()),
        None,
        None,
        true,
    )
    .expect("PTY run");
    let marked = crate::auth_marker::record_from_run(task.id.as_str(), AgentKind::OpenCode, &store);
    assert_eq!(
        crate::auth_marker::auth_status(AgentKind::OpenCode, None).failed(),
        marked
    );
    marked
}

#[test]
fn tool_printed_401_success_over_pty_does_not_mark_round2_regression() {
    assert!(!captured_auth(ERROR, 0));
}

#[test]
fn later_assistant_after_401_over_failed_pty_does_not_mark_round2_regression() {
    assert!(!captured_auth(&format!("{ERROR}\n{ASSISTANT}"), 1));
}

#[test]
fn unframed_401_and_tool_envelope_over_failed_pty_do_not_mark_round2_regression() {
    assert!(!captured_auth(&format!("tool printed: {ERROR}"), 1));
    assert!(!captured_auth(
        &format!(r#"{{"type":"tool_result","output":{ERROR}}}"#),
        1
    ));
}

#[test]
fn real_401_failure_over_pty_marks_round2_regression() {
    assert!(captured_auth(ERROR, 1));
}

#[test]
fn raw_401_log_without_decoded_event_does_not_mark_round2_regression() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("directories");
    let store = Store::open_memory().expect("store");
    let mut task = crate::pty_watch::tests::pty_task("t-log-only", TaskStatus::Failed);
    task.agent = AgentKind::OpenCode;
    task.exit_code = Some(1);
    store.insert_task(&task).expect("task");
    std::fs::write(paths::log_path(task.id.as_str()), ERROR).expect("raw log");
    assert!(!crate::auth_marker::record_from_run(
        task.id.as_str(),
        task.agent,
        &store
    ));
}

fn wrap_fixture() -> (
    tempfile::TempDir,
    AidHomeGuard,
    Arc<Store>,
    crate::types::TaskId,
    PtyBridge,
) {
    let home = tempfile::tempdir().expect("home");
    let guard = AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("directories");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut task = crate::pty_watch::tests::pty_task("t-pty-wrap", TaskStatus::Running);
    task.agent = AgentKind::Gemini;
    store.insert_task(&task).expect("task");
    let bridge = PtyBridge::spawn(&["/bin/cat".into()], None, vec![]).expect("PTY");
    (home, guard, store, task.id, bridge)
}

fn consume_twice(store: &Arc<Store>, id: &crate::types::TaskId, bridge: &mut PtyBridge) {
    let mut state = MonitorState::new(true, None);
    state
        .maybe_consume_reply(bridge, store, id)
        .expect("pending delivery");
    state
        .maybe_forward_steer(bridge, store, id)
        .expect("next signal poll");
    state
        .maybe_consume_reply(bridge, store, id)
        .expect("next pending poll");
    let events = store.get_events(id.as_str()).expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_kind == EventKind::Reasoning
                && e.detail.contains("written to PTY; consumption unconfirmed"))
            .count(),
        1
    );
    let messages = store.list_messages_for_task(id.as_str()).expect("messages");
    assert_eq!(messages.len(), 1);
    assert!(messages[0].delivered_at.is_some());
}

#[test]
fn monitor_races_wrap_up_producer_delivers_once_round2_regression() {
    let _permit = crate::test_subprocess::acquire();
    let (_home, _guard, store, id, mut bridge) = wrap_fixture();
    let mut state = MonitorState::new(true, None);
    state
        .maybe_forward_steer(&mut bridge, &store, &id)
        .expect("empty signal poll");
    assert!(
        crate::cmd::steer::wrap_up_before_timeout(&store, id.as_str(), 540, 600).expect("queue")
    );
    consume_twice(&store, &id, &mut bridge);
    bridge.kill().expect("kill");
    bridge.wait().expect("wait");
}

#[test]
fn failed_commit_leaves_no_signal_and_retry_delivers_once_round2_regression() {
    let _permit = crate::test_subprocess::acquire();
    let (_home, _guard, store, id, mut bridge) = wrap_fixture();
    store.db().execute_batch(
        "CREATE TABLE commit_failure (task_id TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED);
         CREATE TRIGGER fail_wrap_commit AFTER INSERT ON task_messages BEGIN
           INSERT INTO commit_failure VALUES ('missing-task'); END;",
    ).expect("commit fault");
    assert!(crate::cmd::steer::wrap_up_before_timeout(&store, id.as_str(), 540, 600).is_err());
    assert!(store.get_events(id.as_str()).expect("events").is_empty());
    assert!(
        store
            .pending_inbound_for_task(id.as_str())
            .expect("pending")
            .is_empty()
    );
    assert!(
        !paths::steer_signal_path(id.as_str()).exists(),
        "failed commit exposed a signal"
    );
    store
        .db()
        .execute_batch("DROP TRIGGER fail_wrap_commit;")
        .expect("repair");
    assert!(
        crate::cmd::steer::wrap_up_before_timeout(&store, id.as_str(), 541, 600).expect("retry")
    );
    consume_twice(&store, &id, &mut bridge);
    assert!(
        !crate::cmd::steer::wrap_up_before_timeout(&store, id.as_str(), 542, 600).expect("poll")
    );
    bridge.kill().expect("kill");
    bridge.wait().expect("wait");
}

#[test]
fn failed_message_insert_can_retry_once_round2_regression() {
    let _permit = crate::test_subprocess::acquire();
    let (_home, _guard, store, id, mut bridge) = wrap_fixture();
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER fail_wrap_insert BEFORE INSERT ON task_messages BEGIN
           SELECT RAISE(ABORT, 'synthetic publication failure'); END;",
        )
        .expect("insert fault");
    assert!(crate::cmd::steer::wrap_up_before_timeout(&store, id.as_str(), 540, 600).is_err());
    assert!(store.get_events(id.as_str()).expect("events").is_empty());
    assert!(
        store
            .pending_inbound_for_task(id.as_str())
            .expect("pending")
            .is_empty()
    );
    assert!(!paths::steer_signal_path(id.as_str()).exists());
    store
        .db()
        .execute_batch("DROP TRIGGER fail_wrap_insert;")
        .expect("repair");
    assert!(
        crate::cmd::steer::wrap_up_before_timeout(&store, id.as_str(), 541, 600).expect("retry")
    );
    consume_twice(&store, &id, &mut bridge);
    bridge.kill().expect("kill");
    bridge.wait().expect("wait");
}

#[test]
fn operator_steer_remains_allowed_for_read_only_round2_regression() {
    let _permit = crate::test_subprocess::acquire();
    let (_home, _guard, store, id, mut bridge) = wrap_fixture();
    store
        .db()
        .execute(
            "UPDATE tasks SET read_only = 1 WHERE id = ?1",
            [id.as_str()],
        )
        .expect("read-only");
    assert!(
        !crate::cmd::steer::wrap_up_before_timeout(&store, id.as_str(), 540, 600).expect("auto")
    );
    crate::cmd::steer::run(&store, id.as_str(), "operator guidance").expect("operator steer");
    MonitorState::new(true, None)
        .maybe_forward_steer(&mut bridge, &store, &id)
        .expect("operator signal");
    consume_twice(&store, &id, &mut bridge);
    bridge.kill().expect("kill");
    bridge.wait().expect("wait");
}
