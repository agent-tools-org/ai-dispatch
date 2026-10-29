// Tests for nested-agent delegation constraints.
// Covers parent fill, depth refusal, sync requirement, and profile ceilings.
// Deps: run_delegation helpers, Store; parent and depth are passed in, not read from env.

use super::{apply_delegation_from, task_depth, MAX_TASK_DEPTH};
use crate::cmd::run::RunArgs;
use crate::store::Store;
use crate::types::{Task, TaskBudget, TaskDifficulty, TaskId, TaskProfileDeclaration, TaskStatus};

fn make_task(id: &str, parent: Option<&str>) -> Task {
    Task {
        prompt: "test".to_string(),
        parent_task_id: parent.map(str::to_string),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Running)
    }
}

#[test]
fn fills_parent_from_aid_task_id_and_forces_sync() {
    let store = Store::open_memory().expect("store");
    store.insert_task(&make_task("t-parent", None)).expect("insert");
    let mut args = RunArgs {
        background: true,
        ..Default::default()
    };
    let err = apply_delegation_from(&store, &mut args, Some("t-parent"), Some("0"))
        .expect_err("bg must fail");
    assert!(err.to_string().contains("--bg"));
    assert_eq!(args.parent_task_id.as_deref(), Some("t-parent"));
}

#[test]
fn refuses_dispatch_beyond_max_depth() {
    let store = Store::open_memory().expect("store");
    store.insert_task(&make_task("t-root", None)).expect("insert");
    store
        .insert_task(&make_task("t-mid", Some("t-root")))
        .expect("insert");
    store
        .insert_task(&make_task("t-leaf", Some("t-mid")))
        .expect("insert");
    let depth = MAX_TASK_DEPTH.to_string();
    let mut args = RunArgs::default();
    let err = apply_delegation_from(&store, &mut args, Some("t-leaf"), Some(&depth))
        .expect_err("depth 3 must fail");
    assert!(err.to_string().contains("depth"));
}

#[test]
fn refuses_child_difficulty_or_budget_above_parent() {
    let store = Store::open_memory().expect("store");
    store.insert_task(&make_task("t-parent", None)).expect("insert");
    store
        .update_task_profile(
            "t-parent",
            TaskProfileDeclaration {
                difficulty: Some(TaskDifficulty::Simple),
                budget: Some(TaskBudget::Cheap),
                urgency: None,
                rigor: None,
            },
        )
        .expect("profile");
    let mut args = RunArgs {
        declared_difficulty: Some(TaskDifficulty::Complex),
        declared_budget: Some(TaskBudget::Cheap),
        ..Default::default()
    };
    let err = apply_delegation_from(&store, &mut args, Some("t-parent"), Some("0"))
        .expect_err("difficulty must fail");
    assert!(err.to_string().contains("difficulty"));

    args.declared_difficulty = Some(TaskDifficulty::Simple);
    args.declared_budget = Some(TaskBudget::Premium);
    let err = apply_delegation_from(&store, &mut args, Some("t-parent"), Some("0"))
        .expect_err("budget must fail");
    assert!(err.to_string().contains("budget"));
}

#[test]
fn task_depth_counts_ancestors() {
    let store = Store::open_memory().expect("store");
    store.insert_task(&make_task("t-root", None)).expect("insert");
    store
        .insert_task(&make_task("t-child", Some("t-root")))
        .expect("insert");
    assert_eq!(task_depth(&store, "t-root").expect("depth"), 0);
    assert_eq!(task_depth(&store, "t-child").expect("depth"), 1);
}

#[test]
fn caller_owned_gates_nested_delegation_before_parent_assignment() {
    let store = Store::open_memory().expect("store");
    let task = make_task("t-owned-parent", None);
    let policy = crate::store::execution_protocol::ExecutionPolicy {
        name: "caller-owned".into(),
        version: 1,
        agent: "codex".into(),
        model: "exact".into(),
        accepted_at: "2026-10-29T10:00:00Z".into(),
    };
    store
        .insert_task_with_policy(
            &task,
            crate::types::DispatchOrigin::Automatic,
            Some(&policy),
        )
        .expect("parent");
    let mut args = RunArgs::default();
    assert!(
        apply_delegation_from(&store, &mut args, Some("t-owned-parent"), None)
            .expect_err("policy refuses nesting")
            .to_string()
            .contains("caller-owned")
    );
    assert!(args.parent_task_id.is_none());
}
