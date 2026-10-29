// Capacity continuation must not enter recovery for a caller-owned task.
// Malformed task data makes entry beyond the policy boundary fail deterministically.
use super::*;

#[tokio::test]
async fn caller_owned_gates_capacity_routing() {
    let (store, id, args) = crate::cmd::run::caller_owned_tests::setup();
    store.db().execute("UPDATE tasks SET execution_policy=NULL WHERE id=?1", [id.as_str()])
        .expect("policy not re-read");
    let mut run = LifecycleRun::new(
        LifecycleMode::Background,
        &store,
        &id,
        &args,
        "codex",
        None,
        None,
        &[],
    );
    assert!(matches!(
        run.settle_capacity(None, None).await.expect("policy gate"),
        ControlFlow::Continue(())
    ));
}
