// Batch regression cases relocated to keep test modules bounded.
// Deps: shared fixtures and imports from the parent test module.
use super::*;

#[test]
fn task_to_run_args_includes_shared_dir_env() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            agent: "codex".to_string(),
            prompt: "test".to_string(),
            ..Default::default()
        },
        &[],
        true,
        &store,
        Some("/tmp/shared-batch"),
    );

    assert_eq!(
        run_args
            .env
            .as_ref()
            .and_then(|env| env.get("AID_SHARED_DIR"))
            .map(String::as_str),
        Some("/tmp/shared-batch")
    );
}

#[test]
fn task_to_run_args_copies_existing_task_id_and_run_flags() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            id: Some("audit-utilcap".to_string()),
            agent: "codex".to_string(),
            prompt: "test".to_string(),
            peer_review: Some("grok".to_string()),
            retry: Some(2),
            best_of: Some(3),
            metric: Some("cargo test".to_string()),
            skills: Some(vec!["implementer".to_string()]),
            on_done: Some("notify done".to_string()),
            sandbox: true,
            no_skill: true,
            ..Default::default()
        },
        &[],
        true,
        &store,
        None,
    );

    assert_eq!(
        run_args.existing_task_id.as_ref().map(|id| id.as_str()),
        Some("audit-utilcap")
    );
    assert_eq!(run_args.peer_review.as_deref(), Some("grok"));
    assert_eq!(run_args.retry, 2);
    assert_eq!(run_args.best_of, Some(3));
    assert_eq!(run_args.metric.as_deref(), Some("cargo test"));
    assert_eq!(run_args.on_done.as_deref(), Some("notify done"));
    assert!(run_args.sandbox);
    assert_eq!(
        run_args.skills,
        vec![crate::cmd::run::NO_SKILL_SENTINEL.to_string()]
    );
}

#[test]
fn task_to_run_args_copies_setup_and_link_deps() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            agent: "codex".to_string(),
            prompt: "test".to_string(),
            worktree: Some("feat/setup".to_string()),
            setup: Some("npm ci".to_string()),
            worktree_link_deps: Some(false),
            ..Default::default()
        },
        &[],
        true,
        &store,
        None,
    );

    assert_eq!(run_args.setup.as_deref(), Some("npm ci"));
    assert!(!run_args.link_deps);
}
