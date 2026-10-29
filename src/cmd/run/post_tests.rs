// Tests for post-run hung retry planning.
// Covers transient retry budgets, cascade selection, and session handling.
// Deps: run_post helpers, hung context metadata, and task domain types.

use super::*;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus};

#[test]
fn maybe_auto_retry_transient_plan_allows_zero_cli_retry() {
    let args = RunArgs { retry: 0, ..Default::default() };
    let context = hung_context(true);
    let task = failed_task(AgentKind::Codex);

    assert_eq!(hung_retry_retries_left(&args, &context, 0), Some(0));
    assert!(run_hung_recovery::should_auto_retry_hung(&task, &context, 0));
}

#[test]
fn maybe_auto_retry_non_transient_plan_still_requires_cli_retry() {
    let args = RunArgs { retry: 0, ..Default::default() };
    let context = hung_context(false);

    assert_eq!(hung_retry_retries_left(&args, &context, 0), None);
}

#[test]
fn transient_hung_retry_args_take_first_cascade_agent_and_clear_session() {
    let repo = tempfile::tempdir().unwrap();
    let mut task = failed_task(AgentKind::OpenCode);
    task.repo_path = Some(repo.path().display().to_string());
    task.agent_session_id = Some("old-session".to_string());
    let args = RunArgs {
        agent_name: "opencode".to_string(),
        retry: 0,
        session_id: Some("old-arg-session".to_string()),
        cascade: vec!["codex".to_string(), "cursor".to_string()],
        ..Default::default()
    };

    let retry_args = build_hung_retry_args(&Store::open_memory().unwrap(), &args, &task, &hung_context(true), "feedback", "root")
        .unwrap();

    assert_eq!(retry_args.agent_name, "codex");
    assert_eq!(retry_args.cascade, vec!["cursor".to_string()]);
    assert_eq!(retry_args.session_id, None);
    assert_eq!(retry_args.parent_task_id.as_deref(), Some(task.id.as_str()));
    assert_eq!(retry_args.retry, 0);
}

#[test]
fn non_transient_hung_retry_args_resume_session_and_decrement_retry() {
    let repo = tempfile::tempdir().unwrap();
    let mut task = failed_task(AgentKind::OpenCode);
    task.repo_path = Some(repo.path().display().to_string());
    task.agent_session_id = Some("resume-session".to_string());
    let args = RunArgs {
        agent_name: "opencode".to_string(),
        retry: 2,
        ..Default::default()
    };

    let retry_args = build_hung_retry_args(&Store::open_memory().unwrap(), &args, &task, &hung_context(false), "feedback", "root")
        .unwrap();

    assert_eq!(retry_args.agent_name, "opencode");
    assert_eq!(retry_args.session_id.as_deref(), Some("resume-session"));
    assert_eq!(retry_args.retry, 1);
}

#[test]
fn prior_hung_retry_count_counts_parent_chain_markers() {
    let store = Store::open_memory().expect("store");
    let parent = failed_task_with_id("t-hung-parent", None);
    let child = failed_task_with_id("t-hung-child", Some(parent.id.as_str()));
    store.insert_task(&parent).expect("insert parent");
    store.insert_task(&child).expect("insert child");
    monitor::insert_hung_retry_event(&store, &parent.id).expect("insert retry marker");

    assert_eq!(prior_hung_retry_count(&store, &child).expect("retry count"), 1);
}

fn hung_context(transient: bool) -> monitor::HungContext {
    monitor::HungContext {
        hung_duration_secs: 180,
        event_count: 1,
        last_event_detail: None,
        transient,
    }
}

fn failed_task(agent: AgentKind) -> Task {
    let mut task = failed_task_with_id("t-hung-post", None);
    task.agent = agent;
    task
}

fn failed_task_with_id(task_id: &str, parent_task_id: Option<&str>) -> Task {
    Task {
        parent_task_id: parent_task_id.map(str::to_string),
        ..crate::types::Task::test_fixture(TaskId(task_id.to_string()), TaskStatus::Failed)
    }
}
