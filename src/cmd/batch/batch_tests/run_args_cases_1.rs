// Batch regression cases relocated to keep test modules bounded.
// Deps: shared fixtures and imports from the parent test module.
use super::*;

#[test]
fn task_to_run_args_copies_context() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            agent: "codex".to_string(),
            prompt: "test".to_string(),
            context: Some(vec!["src/lib.rs".to_string(), "src/main.rs:run".to_string()]),
            ..Default::default()
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
            agent: "codex".to_string(),
            prompt: "test".to_string(),
            result_file: Some("result.md".to_string()),
            ..Default::default()
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
            agent: "codex".to_string(),
            prompt: "test".to_string(),
            checklist: Some(vec!["check item".to_string(), "confirm edge case".to_string()]),
            ..Default::default()
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
            agent: "codex".to_string(),
            prompt: "test".to_string(),
            iterate: Some(3),
            eval: Some("cargo test".to_string()),
            eval_feedback_template: Some("Round {iteration}".to_string()),
            ..Default::default()
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
