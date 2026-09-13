// Batch regression cases relocated to keep test modules bounded.
// Deps: shared fixtures and imports from the parent test module.
use super::*;

#[test]
fn task_to_run_args_includes_shared_dir_env() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            id: None,
            name: None,
            agent: "codex".to_string(),
            team: None,
            prompt: "test".to_string(),
            prompt_file: None,
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None, remote_build: None,
            verify: None,
            setup: None,
            judge: None,
            peer_review: None,
            max_duration_mins: None,
            max_wait_mins: None,
            retry: None,
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            idle_timeout: None,
            best_of: None,
            metric: None,
            context: None,
            checklist: None,
            skills: None,
            on_done: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            sandbox: false,
            no_skill: false,
            difficulty: None, budget: None, urgency: None, rigor: None, egress: None, kind: None,
            audit: None,
            env: None,
            env_forward: None,
            worktree_link_deps: None,
            on_success: None,
            on_fail: None,
            conditional: false,
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
            name: None,
            agent: "codex".to_string(),
            team: None,
            prompt: "test".to_string(),
            prompt_file: None,
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None, remote_build: None,
            verify: None,
            setup: None,
            judge: None,
            peer_review: Some("gemini".to_string()),
            max_duration_mins: None,
            max_wait_mins: None,
            retry: Some(2),
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            idle_timeout: None,
            best_of: Some(3),
            metric: Some("cargo test".to_string()),
            context: None,
            checklist: None,
            skills: Some(vec!["implementer".to_string()]),
            on_done: Some("notify done".to_string()),
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            sandbox: true,
            no_skill: true,
            difficulty: None, budget: None, urgency: None, rigor: None, egress: None, kind: None,
            audit: None,
            env: None,
            env_forward: None,
            worktree_link_deps: None,
            on_success: None,
            on_fail: None,
            conditional: false,
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
    assert_eq!(run_args.peer_review.as_deref(), Some("gemini"));
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
            id: None,
            name: None,
            agent: "codex".to_string(),
            team: None,
            prompt: "test".to_string(),
            prompt_file: None,
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: Some("feat/setup".to_string()),
            group: None,
            container: None, remote_build: None,
            verify: None,
            setup: Some("npm ci".to_string()),
            judge: None,
            peer_review: None,
            max_duration_mins: None,
            max_wait_mins: None,
            retry: None,
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            idle_timeout: None,
            best_of: None,
            metric: None,
            context: None,
            checklist: None,
            skills: None,
            on_done: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            sandbox: false,
            no_skill: false,
            difficulty: None, budget: None, urgency: None, rigor: None, egress: None, kind: None,
            audit: None,
            env: None,
            env_forward: None,
            worktree_link_deps: Some(false),
            on_success: None,
            on_fail: None,
            conditional: false,
        },
        &[],
        true,
        &store,
        None,
    );

    assert_eq!(run_args.setup.as_deref(), Some("npm ci"));
    assert!(!run_args.link_deps);
}
