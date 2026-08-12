// Tests for nested-agent delegation constraints.
// Covers parent fill, depth refusal, sync requirement, and profile ceilings.
// Deps: run_delegation helpers, Store, tempfile env isolation.

use super::{apply_nested_delegation, task_depth, MAX_TASK_DEPTH};
use crate::cmd::run::RunArgs;
use crate::store::Store;
use crate::types::{
    AgentKind, Task, TaskBudget, TaskDifficulty, TaskId, TaskProfileDeclaration, TaskStatus,
    VerifyStatus,
};
use chrono::Local;

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: tests run serially for these keys within this module.
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

fn make_task(id: &str, parent: Option<&str>) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "test".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Running,
        parent_task_id: parent.map(str::to_string),
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn fills_parent_from_aid_task_id_and_forces_sync() {
    let store = Store::open_memory().expect("store");
    store.insert_task(&make_task("t-parent", None)).expect("insert");
    let _id = EnvGuard::set("AID_TASK_ID", "t-parent");
    let _depth = EnvGuard::set("AID_TASK_DEPTH", "0");
    let mut args = RunArgs {
        background: true,
        ..Default::default()
    };
    let err = apply_nested_delegation(&store, &mut args).expect_err("bg must fail");
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
    let _id = EnvGuard::set("AID_TASK_ID", "t-leaf");
    let _depth = EnvGuard::set("AID_TASK_DEPTH", &MAX_TASK_DEPTH.to_string());
    let mut args = RunArgs::default();
    let err = apply_nested_delegation(&store, &mut args).expect_err("depth 3 must fail");
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
    let _id = EnvGuard::set("AID_TASK_ID", "t-parent");
    let _depth = EnvGuard::set("AID_TASK_DEPTH", "0");
    let mut args = RunArgs {
        declared_difficulty: Some(TaskDifficulty::Complex),
        declared_budget: Some(TaskBudget::Cheap),
        ..Default::default()
    };
    let err = apply_nested_delegation(&store, &mut args).expect_err("difficulty must fail");
    assert!(err.to_string().contains("difficulty"));

    args.declared_difficulty = Some(TaskDifficulty::Simple);
    args.declared_budget = Some(TaskBudget::Premium);
    let err = apply_nested_delegation(&store, &mut args).expect_err("budget must fail");
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
