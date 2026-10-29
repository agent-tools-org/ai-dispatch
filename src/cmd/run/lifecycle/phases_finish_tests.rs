// Persisted caller-owned policy gates completion-time peer review.
// Invalid task data proves the gate precedes review task lookup and agent invocation.
use super::*;

#[tokio::test]
async fn caller_owned_gates_peer_review() {
    let (store, id, mut args) = crate::cmd::run::caller_owned_tests::setup();
    args.peer_review = Some("agy".into());
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
    run.peer_review().await.expect("gated before task decoding");
}
