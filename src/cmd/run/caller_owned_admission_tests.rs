// Admission regressions for normalized identity and non-consuming dry-run refusal.
// Uses store claims directly so no external agent executable is needed.
use super::*;
use crate::types::AgentKind;
use std::sync::Arc;

#[test]
fn caller_owned_agent_case_and_aliases_share_the_same_idempotent_claim() {
    for (typed, repeated, canonical, kind) in [
        ("Codex", "CODEX", "codex", AgentKind::Codex),
        ("Antigravity", "AGY", "agy", AgentKind::Antigravity),
    ] {
        let store = Store::open_memory().expect("store");
        let mut task = Task::test_fixture(TaskId("t-normalized".into()), crate::types::TaskStatus::Pending);
        task.agent = kind;
        task.requested_model = Some("exact-model".into());
        let mut args = RunArgs {
            policy: Some("caller-owned".into()),
            agent_name: typed.into(),
            model: task.requested_model.clone(),
            existing_task_id: Some(task.id.clone()),
            ..Default::default()
        };
        claim(&store, &task, &args).expect("first claim");
        let facts = store
            .execution_facts(task.id.as_str())
            .expect("facts")
            .expect("row");
        assert_eq!(facts.policy.expect("policy").agent, canonical);
        let revision = facts.revision;
        args.agent_name = repeated.into();
        assert_eq!(
            existing(&store, &args).expect("same execution"),
            Some(task.id.clone())
        );
        let error = claim(&store, &task, &args).expect_err("duplicate claim");
        assert!(error.downcast_ref::<Existing>().is_some());
        assert_eq!(
            store
                .execution_facts(task.id.as_str())
                .expect("facts")
                .expect("row")
                .revision,
            revision
        );
    }
}

#[tokio::test]
async fn caller_owned_dry_run_refuses_before_consuming_or_changing_an_id() {
    let store = Arc::new(Store::open_memory().expect("store"));
    let args = RunArgs {
        policy: Some("caller-owned".into()),
        agent_name: "codex".into(),
        existing_task_id: Some(TaskId("t-no-dry-run".into())),
        model: Some("exact-model".into()),
        dry_run: true,
        ..Default::default()
    };
    let error = crate::cmd::run::run(store.clone(), args.clone())
        .await
        .expect_err("refuse dry run");
    let refusal = error
        .downcast_ref::<crate::execution::Refusal>()
        .expect("protocol refusal");
    assert_eq!(
        (refusal.code, refusal.exit, refusal.detail.as_deref()),
        ("flag_refused", 3, Some("dry_run"))
    );
    assert!(store.get_task("t-no-dry-run").expect("query").is_none());
    let task = Task::test_fixture(TaskId("t-no-dry-run".into()), crate::types::TaskStatus::Pending);
    store.insert_task(&task).expect("reuse refused ID");
    let before = store.execution_snapshot(task.id.as_str()).expect("snapshot");
    assert!(crate::cmd::run::run(store.clone(), args).await.is_err());
    assert_eq!(
        store.execution_snapshot(task.id.as_str()).expect("snapshot"),
        before
    );
}

#[tokio::test]
async fn caller_owned_container_refusal_preserves_an_unused_id_and_existing_rows() {
    let store = Arc::new(Store::open_memory().expect("store"));
    let args = RunArgs {
        policy: Some("caller-owned".into()), existing_task_id: Some(TaskId("t-no-container".into())),
        model: Some("exact".into()), container: Some("dev:latest".into()), ..Default::default()
    };
    let error = crate::cmd::run::run(store.clone(), args.clone()).await.expect_err("refuse");
    let refusal = error.downcast_ref::<crate::execution::Refusal>().expect("protocol error");
    assert_eq!((refusal.code, refusal.exit, refusal.detail.as_deref()), ("flag_refused", 3, Some("container")));
    assert!(store.get_task("t-no-container").expect("query").is_none());
    let task = Task::test_fixture(TaskId("t-no-container".into()), crate::types::TaskStatus::Pending);
    store.insert_task(&task).expect("reuse unused ID");
    let before = store.execution_snapshot(task.id.as_str()).expect("snapshot");
    assert!(crate::cmd::run::run(store.clone(), args).await.is_err());
    assert_eq!(store.execution_snapshot(task.id.as_str()).expect("snapshot"), before);
}

#[tokio::test]
async fn caller_owned_hook_refusals_do_not_consume_task_ids() {
    for hook in [false,true] {
        let store=Arc::new(Store::open_memory().expect("store"));
        let mut args=RunArgs { policy:Some("caller-owned".into()),existing_task_id:Some(TaskId("t-no-hooks".into())),
            model:Some("exact".into()),..Default::default() };
        if hook {args.hooks=vec!["on_done:true".into()];} else {args.on_done=Some("true".into());}
        let error=crate::cmd::run::run(store.clone(),args).await.expect_err("refuse hook");
        let refusal=error.downcast_ref::<crate::execution::Refusal>().expect("refusal");
        assert_eq!((refusal.code,refusal.exit,refusal.detail.as_deref()),("flag_refused",3,Some(if hook {"hook"} else {"on_done"})));
        assert!(store.get_task("t-no-hooks").expect("query").is_none());
    }
}
