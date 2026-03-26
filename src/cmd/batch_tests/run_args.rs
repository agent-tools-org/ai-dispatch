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
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            max_duration_mins: None,
            idle_timeout: None,
            context: Some(vec!["src/lib.rs".to_string(), "src/main.rs:run".to_string()]),
            checklist: None,
            skills: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            budget: false,
            env: None,
            env_forward: None,
            judge: None,
            best_of: None,
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
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            max_duration_mins: None,
            idle_timeout: None,
            context: None,
            checklist: Some(vec!["check item".to_string(), "confirm edge case".to_string()]),
            skills: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            budget: false,
            env: None,
            env_forward: None,
            judge: None,
            best_of: None,
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
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            max_duration_mins: None,
            idle_timeout: None,
            context: None,
            checklist: None,
            skills: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            budget: false,
            env: None,
            env_forward: None,
            judge: None,
            best_of: None,
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
        model: None,
        worktree: None,
        group: None,
        container: None,
        verify: None,
        max_duration_mins: None,
        idle_timeout: None,
        context: None,
        checklist: None,
        skills: None,
        hooks: None,
        depends_on: None,
        parent: None,
        context_from: None,
        fallback: None,
        scope: None,
        read_only: false,
        budget: false,
        env: None,
        env_forward: None,
        judge: None,
        best_of: None,
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
        model: None,
        worktree: None,
        group: None,
        container: None,
        verify: None,
        max_duration_mins: None,
        idle_timeout: None,
        context: None,
        checklist: None,
        skills: None,
        hooks: None,
        depends_on: None,
        parent: None,
        context_from: None,
        fallback: None,
        scope: None,
        read_only: false,
        budget: false,
        env: None,
        env_forward: None,
        judge: None,
        best_of: None,
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
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            max_duration_mins: None,
            idle_timeout: None,
            context: None,
            checklist: None,
            skills: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            budget: false,
            env: Some([("PATH".to_string(), "explicit".to_string())].into_iter().collect()),
            env_forward: Some(vec!["PATH".to_string()]),
            judge: None,
            best_of: None,
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
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            max_duration_mins: None,
            idle_timeout: None,
            context: None,
            checklist: None,
            skills: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            budget: false,
            env: None,
            env_forward: None,
            judge: None,
            best_of: None,
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
fn task_to_run_args_copies_existing_task_id() {
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
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            max_duration_mins: None,
            idle_timeout: None,
            context: None,
            checklist: None,
            skills: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            budget: false,
            env: None,
            env_forward: None,
            judge: None,
            best_of: None,
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
}
