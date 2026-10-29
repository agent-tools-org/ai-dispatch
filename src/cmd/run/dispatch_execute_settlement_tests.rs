// A spawned worker owns settlement even if launch publication or observation fails.
// Exercises the launch boundary with real store write failures and existing spawn fixtures.
use super::*;

fn persist_policy(store: &Store) {
    let policy = crate::store::execution_protocol::ExecutionPolicy {
        name: "caller-owned".into(),
        version: 1,
        agent: "codex".into(),
        model: "exact".into(),
        accepted_at: "2026-10-29T10:00:00Z".into(),
    };
    store
        .db()
        .execute(
            "UPDATE tasks SET execution_policy = ?1 WHERE id = 't-launch'",
            [serde_json::to_string(&policy).expect("policy")],
        )
        .expect("policy");
}

#[test]
fn caller_owned_running_state_write_failure_does_not_settle_a_spawned_worker() {
    let (_home, _guard, store, prepared, spec) = setup();
    persist_policy(&store);
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER reject_running BEFORE UPDATE OF worker_settlement ON tasks
         WHEN json_extract(NEW.worker_settlement, '$.state') = 'running'
         BEGIN SELECT RAISE(ABORT, 'running state unavailable'); END;",
        )
        .expect("inject post-spawn observation failure");
    SPAWN.with(|slot| *slot.borrow_mut() = Some(Box::new(|| Ok(std::process::id()))));
    launch_background_worker(&store, &prepared, &spec).expect("worker launch remains accepted");
    let row = store.get_task("t-launch").expect("task").expect("row");
    assert_eq!(row.status, TaskStatus::Running);
    assert!(
        store
            .execution_facts("t-launch")
            .expect("facts")
            .expect("row")
            .settlement
            .is_none()
    );
    assert_eq!(
        background::load_worker_pid("t-launch").expect("worker"),
        Some(std::process::id())
    );
}

#[test]
fn caller_owned_post_spawn_failure_keeps_worker_barrier_and_never_claims_launch_failed() {
    let (_home, _guard, store, prepared, spec) = setup();
    persist_policy(&store);
    SPAWN.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(|| {
            background::clear_spec("t-launch")?;
            std::fs::create_dir(crate::support::paths::job_path("t-launch"))?;
            Ok(u32::MAX)
        }))
    });
    let error = launch_background_worker(&store, &prepared, &spec).expect_err("PID publication failure");
    assert!(worker_was_spawned(&error));
    assert!(store.execution_process("t-launch").expect("spawn evidence").1);
    assert!(
        crate::support::paths::job_path("t-launch").is_dir(),
        "barrier retained"
    );
    assert_eq!(
        store.get_task("t-launch").expect("task").expect("row").status,
        TaskStatus::Running
    );
    assert!(
        store
            .execution_facts("t-launch")
            .expect("facts")
            .expect("row")
            .settlement
            .is_none()
    );
}

#[test]
fn default_post_spawn_failure_does_not_claim_launch_failed_and_retains_cleanup() {
    let (_home, _guard, store, prepared, spec) = setup();
    SPAWN.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(|| {
            background::clear_spec("t-launch")?;
            Ok(u32::MAX)
        }))
    });
    let error = launch_background_worker(&store, &prepared, &spec).expect_err("PID publication failure");
    assert!(worker_was_spawned(&error));
    assert!(store.execution_process("t-launch").expect("spawn evidence").1);
    assert!(!crate::support::paths::job_path("t-launch").exists());
    assert_eq!(
        store.get_task("t-launch").expect("task").expect("row").status,
        TaskStatus::Failed
    );
    assert!(
        store
            .execution_facts("t-launch")
            .expect("facts")
            .expect("row")
            .settlement
            .is_none()
    );
}
