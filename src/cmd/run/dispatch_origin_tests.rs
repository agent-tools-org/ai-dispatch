// Dispatch origin and complete retry inputs at the initial task claim.
// Deps: dispatch preparation, saved RunArgs and real Store mutations.
use super::*;

#[path = "verify_gate_fixture.rs"]
mod fixture;

#[test]
fn explicit_retry_claim_saves_complete_configuration_before_worktree_setup() {
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let parent = fixture::task("t-claim-parent", TaskStatus::Stopped, None, None);
    store.insert_task(&parent).unwrap();
    let mut args = RunArgs {
        agent_name: "codex".into(),
        prompt: "Retry with feedback".into(),
        dir: Some(home.path().display().to_string()),
        parent_task_id: Some(parent.id.to_string()),
        origin: DispatchOrigin::ExplicitRetry,
        verify: Some("echo verify".into()),
        hooks: vec!["on_done:true".into()],
        read_only: true,
        dry_run: true,
        ..Default::default()
    };
    let context = resolve_dispatch_context(&store, &mut args).unwrap();

    let claimed = claim_dispatch(&store, &mut args, &context, |_| true).unwrap();

    let saved = RunArgs::saved_for_task(&store, claimed.task_id.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(saved.origin, DispatchOrigin::ExplicitRetry);
    assert_eq!(saved.agent_name, args.agent_name);
    assert_eq!(saved.prompt, args.prompt);
    assert_eq!(saved.dir, args.dir);
    assert_eq!(saved.verify, args.verify);
    assert_eq!(saved.hooks, args.hooks);
    assert!(saved.env.is_none());
    let retry = RunArgs::for_retry(&store, &claimed.task).unwrap();
    assert_eq!(retry.origin, DispatchOrigin::Automatic);
    assert_eq!(retry.verify, args.verify);
    assert_eq!(retry.hooks, args.hooks);
}
