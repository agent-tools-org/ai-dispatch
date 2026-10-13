// Post-run failure regressions against an already settled task winner.
// Exercises verification, report delivery and real checkout settlement callbacks.
// Deps: lifecycle steps, stop command, isolated Store and temporary Git fixtures.

use super::*;
use crate::store::TaskCompletionUpdate;

#[path = "settlement_notice_tests.rs"]
mod notices;

struct Fixture {
    home: tempfile::TempDir,
    _guard: crate::paths::AidHomeGuard,
    store: Arc<Store>,
    id: TaskId,
    winner: Task,
}

impl Fixture {
    fn new(status: TaskStatus) -> Self {
        let home = tempfile::tempdir().expect("home");
        let guard = crate::paths::AidHomeGuard::set(home.path());
        crate::paths::ensure_dirs().expect("dirs");
        let store = Arc::new(Store::open_memory().expect("store"));
        let mut task = crate::cost::test_support::task(AgentKind::Codex, 0, None);
        task.status = TaskStatus::Running;
        store.insert_task(&task).expect("running task");
        store
            .insert_event(&TaskEvent {
                task_id: task.id.clone(),
                timestamp: chrono::Local::now(),
                event_kind: EventKind::Error,
                detail: "original failure reason".into(),
                metadata: None,
            })
            .expect("original reason");
        if status == TaskStatus::Stopped {
            crate::cmd::stop::stop(&store, task.id.as_str()).expect("stop wins");
        } else {
            assert!(
                store
                    .update_task_completion(TaskCompletionUpdate {
                        id: task.id.as_str(),
                        status,
                        tokens: None,
                        duration_ms: 77,
                        observed_model: None,
                        attribution_source: None,
                        cost_usd: None,
                        exit_code: Some(0),
                    })
                    .expect("settlement")
            );
        }
        let winner = store
            .get_task(task.id.as_str())
            .expect("query")
            .expect("winner");
        Self {
            home,
            _guard: guard,
            store,
            id: task.id,
            winner,
        }
    }

    fn assert_unchanged(&self) {
        let task = self
            .store
            .get_task(self.id.as_str())
            .expect("query")
            .expect("task");
        assert_eq!(task.status, self.winner.status);
        assert_eq!(task.duration_ms, self.winner.duration_ms);
        assert_eq!(task.completed_at, self.winner.completed_at);
        assert_eq!(
            self.store.latest_error(self.id.as_str()).as_deref(),
            Some("original failure reason")
        );
        let events = self.store.get_events(self.id.as_str()).expect("events");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_kind == EventKind::Error)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_kind == EventKind::Completion)
                .count(),
            usize::from(self.winner.status == TaskStatus::Stopped)
        );
    }
}

#[test]
fn verification_failure_preserves_settlement_winner() {
    let _permit = crate::test_subprocess::acquire();
    for status in [TaskStatus::Stopped, TaskStatus::Done, TaskStatus::Failed] {
        let f = Fixture::new(status);
        maybe_verify(&f.store, &f.id, Some("false"), f.home.path().to_str(), None);
        assert_eq!(
            f.store
                .get_task(f.id.as_str())
                .expect("query")
                .expect("task")
                .verify_status,
            VerifyStatus::Failed,
            "verification metadata remains observational"
        );
        f.assert_unchanged();
    }
}

#[test]
fn missing_verify_directory_preserves_settlement_winner() {
    let f = Fixture::new(TaskStatus::Stopped);
    let missing = f.home.path().join("missing");
    maybe_verify(&f.store, &f.id, Some("false"), missing.to_str(), None);
    f.assert_unchanged();
}

#[test]
fn missing_report_preserves_settlement_winner() {
    for status in [TaskStatus::Stopped, TaskStatus::Done, TaskStatus::Failed] {
        let f = Fixture::new(status);
        record_missing_report(
            &f.store,
            &f.id,
            run_prompt::ResultDelivery::MissingFile {
                fallback_saved: false,
            },
            true,
        );
        f.assert_unchanged();
        assert_eq!(
            f.store
                .get_task(f.id.as_str())
                .expect("query")
                .expect("task")
                .delivery_assessment,
            Some(DeliveryAssessment::MissingFinalDelivery)
        );
    }
}

#[test]
fn artifact_failure_preserves_settlement_winner_and_visible_notice() {
    for status in [TaskStatus::Stopped, TaskStatus::Done, TaskStatus::Failed] {
        let f = Fixture::new(status);
        record_artifact_failure(
            &f.store,
            &f.winner,
            true,
            &anyhow::anyhow!("synthetic publication failure"),
        );
        f.assert_unchanged();
        let events = f.store.get_events(f.id.as_str()).expect("events");
        let event = events
            .iter()
            .find(|event| event.detail.contains("synthetic publication failure"))
            .expect("publication notice remains visible");
        assert_eq!(event.event_kind, EventKind::Milestone);
    }
}

#[test]
fn stale_artifact_snapshot_does_not_authorize_losing_failure() {
    let f = Fixture::new(TaskStatus::Stopped);
    let mut stale = f.winner.clone();
    stale.status = TaskStatus::Done;
    stale.completed_at = None;
    record_artifact_failure(
        &f.store,
        &stale,
        false,
        &anyhow::anyhow!("synthetic copy failure"),
    );
    f.assert_unchanged();
}

#[tokio::test]
async fn read_only_violation_preserves_settlement_winner() {
    let _permit = crate::test_subprocess::acquire();
    let f = Fixture::new(TaskStatus::Stopped);
    init_repo(f.home.path());
    let dir = f.home.path().to_str().expect("dir");
    let baseline = super::super::super::run_dirty::capture_baseline(&f.store, &f.id, Some(dir))
        .expect("baseline")
        .expect("Git baseline");
    std::fs::write(f.home.path().join("file.txt"), "forbidden edit\n").expect("agent edit");
    let args = RunArgs {
        read_only: true,
        ..RunArgs::default()
    };
    let action = post_agent_dirty_worktree_cleanup(&f.store, &f.id, &args, dir, Some(&baseline))
        .await
        .expect("read-only settlement");
    assert_eq!(action, DirtyWorktreeAction::Failed);
    f.assert_unchanged();
    assert_eq!(
        std::fs::read_to_string(f.home.path().join("file.txt")).expect("file"),
        "forbidden edit\n"
    );
}

#[tokio::test]
async fn shared_checkout_failure_preserves_settlement_winner() {
    let _permit = crate::test_subprocess::acquire();
    let f = Fixture::new(TaskStatus::Stopped);
    init_repo(f.home.path());
    std::fs::write(f.home.path().join(".git/config"), "[broken\n").expect("invalid Git config");
    let action = post_agent_dirty_worktree_cleanup(
        &f.store,
        &f.id,
        &RunArgs::default(),
        f.home.path().to_str().expect("dir"),
        None,
    )
    .await
    .expect("preservation failure");
    assert_eq!(action, DirtyWorktreeAction::Failed);
    f.assert_unchanged();
}

fn init_repo(dir: &Path) {
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.name", "Test User"],
        vec!["config", "user.email", "test@example.com"],
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(dir)
                .args(args)
                .status()
                .expect("git")
                .success()
        );
    }
    std::fs::write(dir.join("file.txt"), "original\n").expect("file");
    for args in [vec!["add", "file.txt"], vec!["commit", "-m", "fixture"]] {
        assert!(
            std::process::Command::new("git")
                .current_dir(dir)
                .args(args)
                .status()
                .expect("git")
                .success()
        );
    }
}

#[test]
fn rejected_transition_notice_preserves_settlement_winner() {
    let f = Fixture::new(TaskStatus::Stopped);
    assert!(
        !f.store
            .update_task_status(f.id.as_str(), TaskStatus::Running)
            .expect("rejected transition")
    );
    f.assert_unchanged();
    let events = f.store.get_events(f.id.as_str()).expect("events");
    let event = events
        .iter()
        .find(|event| event.detail.starts_with("rejected illegal"))
        .expect("rejection stays visible");
    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(
        event.metadata.as_ref().expect("metadata")["from"],
        "stopped"
    );
}

#[test]
fn hung_retry_notice_preserves_settlement_winner() {
    let f = Fixture::new(TaskStatus::Failed);
    crate::process_monitor::insert_hung_retry_event(&f.store, &f.id).expect("retry notice");
    f.assert_unchanged();
    let events = f.store.get_events(f.id.as_str()).expect("events");
    assert!(crate::process_monitor::was_auto_retried_after_hang(&events));
    assert_eq!(
        events.last().expect("notice").event_kind,
        EventKind::Milestone
    );
}
