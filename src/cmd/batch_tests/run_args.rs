// Tests for task_to_run_args conversion details.
// Exports: (tests only)
// Deps: crate::batch, batch_args
use crate::batch;
use crate::store::Store;
use std::sync::Arc;

use super::super::batch_args::task_to_run_args;

#[test]
fn task_to_run_args_copies_context() {
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            id: None,
            name: None,
            agent: "codex".to_string(),
            team: None,
            prompt: "test".to_string(),
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            judge: None,
            peer_review: None,
            max_duration_mins: None,
            retry: None,
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
            budget: false,
            env: None,
            env_forward: None,
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
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            id: None,
            name: None,
            agent: "codex".to_string(),
            team: None,
            prompt: "test".to_string(),
            dir: None,
            output: None,
            result_file: Some("result.md".to_string()),
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            judge: None,
            peer_review: None,
            max_duration_mins: None,
            retry: None,
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
            budget: false,
            env: None,
            env_forward: None,
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
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            id: None,
            name: None,
            agent: "codex".to_string(),
            team: None,
            prompt: "test".to_string(),
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            judge: None,
            peer_review: None,
            max_duration_mins: None,
            retry: None,
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
            budget: false,
            env: None,
            env_forward: None,
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
fn task_to_run_args_defaults_dry_run_to_false() {
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            id: None,
            name: None,
            agent: "codex".to_string(),
            team: None,
            prompt: "test".to_string(),
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            judge: None,
            peer_review: None,
            max_duration_mins: None,
            retry: None,
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
            budget: false,
            env: None,
            env_forward: None,
            on_success: None,
            on_fail: None,
            conditional: false,
        },
        &[],
        false,
        &store,
        None,
    );

    assert!(!run_args.dry_run);
}

#[test]
fn task_to_run_args_includes_sibling_metadata() {
    let store = Arc::new(Store::open_memory().unwrap());
    let current = batch::BatchTask {
        id: Some("task-current".to_string()),
        name: Some("current".to_string()),
        agent: "codex".to_string(),
        team: None,
        prompt: "implement the feature".to_string(),
        dir: None,
        output: None,
        result_file: None,
        model: None,
        worktree: None,
        group: None,
        container: None,
        verify: None,
        judge: None,
        peer_review: None,
        max_duration_mins: None,
        retry: None,
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
        budget: false,
        env: None,
        env_forward: None,
        on_success: None,
        on_fail: None,
        conditional: false,
    };
    let sibling = batch::BatchTask {
        id: Some("task-sibling".to_string()),
        name: Some("sibling".to_string()),
        agent: "gemini".to_string(),
        team: None,
        prompt: "review the implementation".to_string(),
        dir: None,
        output: None,
        result_file: None,
        model: None,
        worktree: None,
        group: None,
        container: None,
        verify: None,
        judge: None,
        peer_review: None,
        max_duration_mins: None,
        retry: None,
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
        budget: false,
        env: None,
        env_forward: None,
        on_success: None,
        on_fail: None,
        conditional: false,
    };

    let run_args = task_to_run_args(&current, &[&sibling], true, &store, None);

    assert_eq!(
        run_args.batch_siblings,
        vec![(
            "sibling".to_string(),
            "gemini".to_string(),
            "review the implementation".to_string(),
        )]
    );
}

#[test]
fn task_to_run_args_applies_forwarded_env_after_explicit_env() {
    let store = Arc::new(Store::open_memory().unwrap());
    let forwarded_path = std::env::var("PATH").unwrap();
    let run_args = task_to_run_args(
        &batch::BatchTask {
            id: None,
            name: None,
            agent: "codex".to_string(),
            team: None,
            prompt: "test".to_string(),
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            judge: None,
            peer_review: None,
            max_duration_mins: None,
            retry: None,
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
            budget: false,
            env: Some([("PATH".to_string(), "explicit".to_string())].into_iter().collect()),
            env_forward: Some(vec!["PATH".to_string()]),
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
        run_args.env.as_ref().and_then(|env| env.get("PATH")).map(String::as_str),
        Some(forwarded_path.as_str())
    );
    assert_eq!(run_args.env_forward, Some(vec!["PATH".to_string()]));
}

#[test]
fn task_to_run_args_includes_shared_dir_env() {
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            id: None,
            name: None,
            agent: "codex".to_string(),
            team: None,
            prompt: "test".to_string(),
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            judge: None,
            peer_review: None,
            max_duration_mins: None,
            retry: None,
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
            budget: false,
            env: None,
            env_forward: None,
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
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            id: Some("audit-utilcap".to_string()),
            name: None,
            agent: "codex".to_string(),
            team: None,
            prompt: "test".to_string(),
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            judge: None,
            peer_review: Some("gemini".to_string()),
            max_duration_mins: None,
            retry: Some(2),
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
            budget: false,
            env: None,
            env_forward: None,
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
