// Production background worker regressions using an interactive spec and terminal-reading shell.
// Backdates the shared execution start to exercise the final seconds without a reaper.

use super::*;
use crate::config::timeout_policy::{TimeoutPolicy, env_with_policy};
use crate::support::paths::AidHomeGuard;
use std::time::{Duration, Instant};

const READ_TERMINAL: &str = concat!(
    "stty -echo; IFS= read -r -t 4 guidance || exit 1; ",
    r#"printf '%s\n' "$guidance" > "$1""#,
);

struct Fixture {
    home: tempfile::TempDir,
    _guard: AidHomeGuard,
    store: Arc<Store>,
    spec: BackgroundRunSpec,
    args: RunArgs,
}

impl Fixture {
    fn new(policy: TimeoutPolicy) -> Self {
        let home = tempfile::tempdir().expect("home");
        let guard = AidHomeGuard::set(home.path());
        paths::ensure_dirs().expect("dirs");
        let store = Arc::new(Store::open_memory().expect("store"));
        store
            .db()
            .execute(
                "INSERT INTO tasks (id, agent, prompt, status, created_at, read_only)
             VALUES ('t-worker-wrap', 'gemini', '', 'running', ?1, 1)",
                [chrono::Local::now().to_rfc3339()],
            )
            .expect("task");
        let spec: BackgroundRunSpec = serde_json::from_value(serde_json::json!({
            "task_id": "t-worker-wrap", "agent_name": "gemini", "prompt": "report",
            "retry": 0, "interactive": true, "read_only": true,
            "max_duration_secs": policy.max_duration.as_secs(),
        }))
        .expect("spec");
        let args = RunArgs {
            agent_name: "gemini".into(),
            env: env_with_policy(None, policy),
            read_only: true,
            ..Default::default()
        };
        Self {
            home,
            _guard: guard,
            store,
            spec,
            args,
        }
    }

    async fn run(&self, script: &str, start: Instant) {
        let mut command = std::process::Command::new("bash");
        command.args(["-c", script, "fake-agent"]);
        command.arg(self.home.path().join("received.txt"));
        super::run_process(
            &self.store,
            &self.spec,
            &self.args,
            agent::get_agent(AgentKind::Gemini).as_ref(),
            command,
            start,
        )
        .await
        .expect("background worker");
    }
}

async fn assert_pty_wrap_up(policy: TimeoutPolicy) {
    let _permit = crate::support::test_subprocess::acquire();
    let fixture = Fixture::new(policy);
    let limit = policy.max_duration.min(policy.hard_cap);
    let start = Instant::now() - (limit - Duration::from_secs(3));
    fixture.run(READ_TERMINAL, start).await;
    assert_received_wrap_up(&fixture);
}

fn assert_received_wrap_up(fixture: &Fixture) {
    let store = &fixture.store;
    let id = &fixture.spec.task_id;
    let messages = store.list_messages_for_task(id).expect("messages");
    assert_eq!(messages.len(), 1);
    assert!(messages[0].delivered_at.is_some());
    let received = std::fs::read_to_string(fixture.home.path().join("received.txt"))
        .expect("fake agent consumed terminal input before deadline");
    assert_eq!(received.trim(), messages[0].content);
    assert!(received.contains("stop investigating"));
    assert!(received.contains("evidence gathered so far"));
    assert!(received.contains("open questions"));
    assert!(!received.contains("commit"));
    assert!(received.contains("no new write permission"));
    let events = store.get_events(id).expect("events");
    assert!(events.iter().any(|event| {
        event
            .detail
            .contains("written to PTY; consumption unconfirmed")
    }));
    assert!(
        !events
            .iter()
            .any(|event| event.detail.contains("hard timeout"))
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.detail.contains("Hard-cap wrap-up"))
            .count(),
        1
    );
    assert!(store.get_task(id).expect("query").expect("task").read_only);
}

#[tokio::test]
async fn worker_requests_wrap_up_without_reaper_at_effective_deadline() {
    assert_pty_wrap_up(TimeoutPolicy {
        max_duration: Duration::from_secs(600),
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn interactive_worker_wrap_up_uses_smaller_hard_cap() {
    assert_pty_wrap_up(TimeoutPolicy {
        max_duration: Duration::from_secs(7200),
        hard_cap: Duration::from_secs(3600),
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn worker_settlement_cancels_wrap_up_timer() {
    let _permit = crate::support::test_subprocess::acquire();
    let fixture = Fixture::new(TimeoutPolicy::default());
    let start = Instant::now();
    fixture.run("exit 0", start).await;
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(
        fixture
            .store
            .list_messages_for_task(&fixture.spec.task_id)
            .expect("messages")
            .is_empty()
    );
}

#[tokio::test]
async fn interactive_worker_enforces_the_shared_execution_start() {
    let _permit = crate::support::test_subprocess::acquire();
    let fixture = Fixture::new(TimeoutPolicy {
        max_duration: Duration::from_secs(600),
        ..Default::default()
    });
    let start = Instant::now() - Duration::from_millis(599_600);
    fixture.run("sleep 3", start).await;
    let task = fixture
        .store
        .get_task(&fixture.spec.task_id)
        .expect("query")
        .expect("task");
    assert_eq!(task.status, crate::types::TaskStatus::Failed);
    assert!(task.duration_ms.expect("duration") >= 599_600);
    assert!(
        fixture
            .store
            .get_events(&fixture.spec.task_id)
            .expect("events")
            .iter()
            .any(|event| event.detail.contains("hard timeout"))
    );
}

#[tokio::test]
async fn worker_timer_waits_until_ninety_percent() {
    let _permit = crate::support::test_subprocess::acquire();
    let fixture = Fixture::new(TimeoutPolicy {
        max_duration: Duration::from_secs(600),
        ..Default::default()
    });
    let earliest = chrono::Local::now() + chrono::Duration::milliseconds(190);
    let start = Instant::now() - Duration::from_millis(539_800);
    fixture.run(READ_TERMINAL, start).await;
    assert_received_wrap_up(&fixture);
    let events = fixture
        .store
        .get_events(&fixture.spec.task_id)
        .expect("events");
    let queued = events
        .iter()
        .find(|event| event.detail.contains("Hard-cap wrap-up"))
        .expect("claim");
    assert!(queued.timestamp >= earliest);
}
