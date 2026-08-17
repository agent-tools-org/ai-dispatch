// Wait-policy tests. Prepaid must return immediately and must not poll.

use super::*;
use crate::paths::{self, AidHomeGuard};
use crate::rate_limit::{is_rate_limited, mark_rate_limited};
use crate::store::Store;
use crate::types::{AgentKind, TaskProfileDeclaration, TaskUrgency};
use std::time::Instant;

fn isolated() -> (tempfile::TempDir, AidHomeGuard) {
    let temp = tempfile::tempdir().expect("tempdir");
    let _ = std::fs::create_dir_all(temp.path().join(".aid"));
    let guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();
    (temp, guard)
}

fn background_store() -> Store {
    let store = Store::open_memory().expect("store");
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
             VALUES ('t-wait', 'opencode', 'prompt', 'pending', '2026-08-05T00:00:00Z')",
            [],
        )
        .expect("insert task");
    store
        .update_task_profile(
            "t-wait",
            TaskProfileDeclaration {
                urgency: Some(TaskUrgency::Background),
                ..Default::default()
            },
        )
        .expect("profile");
    store
}

#[test]
fn wait_decision_refuses_prepaid() {
    let (_temp, _guard) = isolated();
    mark_rate_limited(
        &AgentKind::OpenCode,
        None,
        "APIError: Insufficient balance. Manage your billing here",
    );
    assert!(is_rate_limited(&AgentKind::OpenCode, None));
    match wait_decision(&AgentKind::OpenCode, None) {
        WaitDecision::Refuse { message } => {
            assert!(message.contains("clear-limit opencode"), "{message}");
            assert!(message.contains("prepaid") || message.contains("will not poll"), "{message}");
        }
        other => panic!("expected Refuse, got {other:?}"),
    }
}

#[tokio::test]
async fn wait_for_declared_reset_returns_immediately_on_prepaid() {
    let (_temp, _guard) = isolated();
    mark_rate_limited(
        &AgentKind::OpenCode,
        None,
        "APIError: Insufficient balance. Manage your billing here",
    );
    let store = background_store();
    let started = Instant::now();
    wait_for_declared_reset(&store, "t-wait", AgentKind::OpenCode, None)
        .await
        .expect("wait");
    assert!(
        started.elapsed() < std::time::Duration::from_millis(400),
        "prepaid must not poll: {:?}",
        started.elapsed()
    );
    assert!(is_rate_limited(&AgentKind::OpenCode, None));
}

#[test]
fn wait_decision_ready_when_not_held() {
    let (_temp, _guard) = isolated();
    assert_eq!(
        wait_decision(&AgentKind::Codex, None),
        WaitDecision::Ready
    );
}
