// Persisted-policy gates must run before recovery inspects task execution data.
// Malformed legacy task data makes accidental recovery observable without launching an agent.
use super::*;
use crate::store::execution_protocol::ExecutionPolicy;
use std::sync::Arc;

pub(super) fn setup() -> (Arc<Store>, TaskId, RunArgs) {
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut task = crate::cost::test_support::task(AgentKind::Codex, 0, None);
    task.id = TaskId("t-policy-gate".into());
    task.requested_model = Some("exact-model".into());
    let policy = ExecutionPolicy {
        name: "caller-owned".into(),
        version: 1,
        agent: "codex".into(),
        model: "exact-model".into(),
        accepted_at: "2026-10-29T10:00:00Z".into(),
    };
    store
        .insert_task_with_policy(&task, crate::types::DispatchOrigin::Automatic, Some(&policy))
        .expect("policy row");
    store
        .db()
        .execute(
            "UPDATE tasks SET tokens = 'invalid' WHERE id = ?1",
            [task.id.as_str()],
        )
        .expect("unreadable execution data");
    let args = RunArgs {
        policy: Some("caller-owned".into()),
        retry: 1,
        verify: Some("false".into()),
        checklist: vec!["required".into()],
        judge: Some("agy".into()),
        audit: true,
        ..Default::default()
    };
    (store, task.id, args)
}

#[tokio::test]
async fn caller_owned_gates_model_self_heal_from_persisted_policy() {
    let (store, id, args) = setup();
    assert!(
        run_model_selfheal::maybe_auto_retry_after_model_unavailable(&store, &id, &args)
            .await
            .expect("gated before task decoding")
            .is_none()
    );
}

#[tokio::test]
async fn caller_owned_gates_hang_retry_even_when_retry_is_zero() {
    let (store, id, mut args) = setup();
    args.retry = 0;
    assert!(
        run_post::maybe_auto_retry_after_hang(&store, &id, &args)
            .await
            .expect("gated before task decoding")
            .is_none()
    );
}

#[tokio::test]
async fn caller_owned_gates_delivery_recovery() {
    let (store, id, args) = setup();
    assert!(
        run_delivery_recovery::maybe_auto_recover_missing_delivery(&store, &id, &args)
            .await
            .expect("gated before task decoding")
            .is_none()
    );
}

#[tokio::test]
async fn caller_owned_gates_verify_retry() {
    let (store, id, args) = setup();
    assert!(
        maybe_auto_retry_after_verify_failure(&store, &id, &args, TaskStatus::Done)
            .await
            .expect("gated before task decoding")
            .is_none()
    );
}

#[tokio::test]
async fn caller_owned_gates_judge_before_any_agent_is_invoked() {
    let (store, id, args) = setup();
    assert!(
        maybe_judge_retry(&store, &args, &id)
            .await
            .expect("gated before task decoding")
            .is_none()
    );
}

#[test]
fn caller_owned_gates_post_done_audit() {
    let (store, id, args) = setup();
    run_post::maybe_run_post_done_audit(&store, &id, &args, None, None).expect("gated before task decoding");
}

#[tokio::test]
async fn caller_owned_gates_explicit_retry_before_superseding_parent() {
    let (store, id, _) = setup();
    let error = crate::cmd::retry::retry_task(
        store,
        crate::cmd::retry::RetryArgs {
            task_id: id.0,
            feedback: None,
            feedback_file: None,
            agent: None,
            model: None,
            idle_timeout_secs: None,
            dir: None,
            reset: false,
            bg: false,
        },
        false,
    )
    .await
    .expect_err("explicit retry refused");
    assert!(error.to_string().contains("caller-owned"));
}

#[test]
fn caller_owned_same_id_does_not_suffix_and_conflicts_do_not_mutate() {
    let store = Store::open_memory().expect("store");
    let mut args = RunArgs {
        policy: Some("caller-owned".into()),
        agent_name: "codex".into(),
        model: Some("exact-model".into()),
        existing_task_id: Some(TaskId("t-idempotent".into())),
        ..Default::default()
    };
    let mut task = crate::cost::test_support::task(AgentKind::Codex, 0, None);
    task.id = args.existing_task_id.clone().expect("id");
    task.requested_model = args.model.clone();
    caller_owned::claim(&store, &task, &args).expect("claim");
    assert_eq!(
        caller_owned::existing(&store, &args).expect("same ID"),
        Some(task.id.clone())
    );
    assert!(
        caller_owned::claim(&store, &task, &args)
            .expect_err("existing")
            .downcast_ref::<caller_owned::Existing>()
            .is_some()
    );
    let before = store.execution_snapshot(task.id.as_str()).expect("snapshot");
    args.model = Some("different".into());
    let error = caller_owned::existing(&store, &args).expect_err("conflict");
    assert_eq!(
        error
            .downcast_ref::<crate::execution::Refusal>()
            .expect("refusal")
            .exit,
        4
    );
    assert_eq!(
        store.execution_snapshot(task.id.as_str()).expect("snapshot"),
        before
    );
}

#[tokio::test]
async fn caller_owned_gates_checklist_retry() {
    use crate::cmd::checklist_scan::{ChecklistResult, ChecklistItemResult, ChecklistItemStatus};
    let (store, id, args) = setup();
    let checklist = ChecklistResult {
        items: vec![ChecklistItemResult {
            item: "required".into(),
            status: ChecklistItemStatus::Missing,
        }],
    };
    assert!(
        maybe_auto_retry_after_checklist_miss(&store, &id, &args, Some(&checklist))
            .await
            .expect("gated before task decoding")
            .is_none()
    );
}

#[tokio::test]
async fn caller_owned_gates_iteration_before_eval() {
    let (store, id, args) = setup();
    let config = IterateConfig {
        max_iterations: 2,
        eval_command: "false".into(),
        feedback_template: None,
    };
    assert!(
        maybe_iterate(&store, &id, &args, &config)
            .await
            .expect("gated before task decoding")
            .is_none()
    );
}

#[tokio::test]
async fn caller_owned_gates_retry_and_cascade() {
    let (store, id, mut args) = setup();
    args.cascade = vec!["agy".into()];
    assert!(
        run_lifecycle::retry_or_cascade(&store, &args, &id)
            .await
            .expect("gated before task decoding")
            .is_none()
    );
}

#[test]
fn caller_owned_racing_claims_have_one_winner() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("aid.db");
    Store::open(&path).expect("initialize schema");
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let path = path.clone();
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            let store = Store::open(&path).expect("store");
            let args = RunArgs {
                policy: Some("caller-owned".into()),
                agent_name: "codex".into(),
                model: Some("exact-model".into()),
                existing_task_id: Some(TaskId("t-race".into())),
                ..Default::default()
            };
            let mut task = crate::cost::test_support::task(AgentKind::Codex, 0, None);
            task.id = args.existing_task_id.clone().expect("id");
            task.requested_model = args.model.clone();
            barrier.wait();
            match caller_owned::claim(&store, &task, &args) {
                Ok(()) => true,
                Err(error) => {
                    assert!(
                        error.downcast_ref::<caller_owned::Existing>().is_some(),
                        "{error}"
                    );
                    false
                }
            }
        }));
    }
    assert_eq!(
        threads
            .into_iter()
            .map(|thread| thread.join().expect("join"))
            .filter(|won| *won)
            .count(),
        1
    );
}

#[tokio::test]
async fn caller_owned_gates_best_of_before_claiming_any_row() {
    let store = Arc::new(Store::open_memory().expect("store"));
    let args = RunArgs {
        policy: Some("caller-owned".into()),
        agent_name: "codex".into(),
        model: Some("exact".into()),
        existing_task_id: Some(TaskId("t-bestof".into())),
        best_of: Some(2),
        ..Default::default()
    };
    let error = run(store.clone(), args).await.expect_err("best-of refused");
    assert_eq!(
        error
            .downcast_ref::<crate::execution::Refusal>()
            .expect("refusal")
            .detail
            .as_deref(),
        Some("best_of")
    );
    assert!(
        store
            .list_tasks(crate::types::TaskFilter::All)
            .expect("tasks")
            .is_empty()
    );
}

#[path = "caller_owned_policy_read_tests.rs"]
mod policy_read_tests;
