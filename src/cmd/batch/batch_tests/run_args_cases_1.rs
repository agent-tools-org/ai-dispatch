// Batch regression cases relocated to keep test modules bounded.
// Deps: shared fixtures and imports from the parent test module.
use super::*;

#[test]
fn task_to_run_args_copies_context() {
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
            context: Some(vec!["src/lib.rs".to_string(), "src/main.rs:run".to_string()]),
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
        None,
    );

    assert_eq!(
        run_args.context,
        vec!["src/lib.rs".to_string(), "src/main.rs:run".to_string()]
    );
}

#[test]
fn task_to_run_args_copies_result_file() {
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
            result_file: Some("result.md".to_string()),
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
        None,
    );

    assert_eq!(run_args.result_file.as_deref(), Some("result.md"));
}

#[test]
fn task_to_run_args_copies_checklist() {
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
            checklist: Some(vec!["check item".to_string(), "confirm edge case".to_string()]),
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
        None,
    );

    assert_eq!(
        run_args.checklist,
        vec!["check item".to_string(), "confirm edge case".to_string()]
    );
}

#[test]
fn task_to_run_args_copies_iterate_config() {
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
            iterate: Some(3),
            eval: Some("cargo test".to_string()),
            eval_feedback_template: Some("Round {iteration}".to_string()),
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
        None,
    );

    assert_eq!(run_args.iterate, Some(3));
    assert_eq!(run_args.eval.as_deref(), Some("cargo test"));
    assert_eq!(
        run_args.eval_feedback_template.as_deref(),
        Some("Round {iteration}")
    );
}
