// Regression tests for background reaper terminalization behavior.
// Covers active execution failures and cleaned-list bookkeeping.

use chrono::{Duration, Local};

use super::{check_zombie_tasks_with, save_spec, BackgroundRunSpec};
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};

fn task(id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
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
        created_at: Local::now() - Duration::seconds(5),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: true,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

fn spec(id: &str) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: id.to_string(),
        worker_pid: Some(77),
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: Some(".".to_string()),
        output: None,
        result_file: None,
        model: None,
        verify: None,
        setup: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        max_duration_mins: Some(60),
        idle_timeout_secs: Some(1),
        retry: 0,
        group: None,
        skills: vec![],
        checklist: vec![],
        hooks: vec![],
        template: None,
        worktree: None,
        base_branch: None,
        peer_review: None,
        audit: false,
        scope: vec![],
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: Some(88),
        sandbox: false,
        read_only: false,
        audit_report_mode: false,
        container: None,
        link_deps: true,
        pre_task_dirty_paths: None,
    }
}

struct TestHome {
    _temp: tempfile::TempDir,
    _guard: paths::AidHomeGuard,
}

fn setup_home() -> TestHome {
    let temp = tempfile::tempdir().expect("tempdir");
    let guard = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    TestHome {
        _temp: temp,
        _guard: guard,
    }
}

#[test]
fn reaper_terminalizes_awaiting_input_task_with_dead_worker() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    store
        .insert_task(&task("t-await-dead", TaskStatus::AwaitingInput))
        .expect("insert task");
    save_spec(&spec("t-await-dead")).expect("save spec");

    let cleaned = check_zombie_tasks_with(&store, |_| false).expect("reap");

    assert_eq!(cleaned, vec!["t-await-dead".to_string()]);
    assert_eq!(
        store.get_task("t-await-dead").expect("get").expect("task").status,
        TaskStatus::Failed
    );
}

#[test]
fn reaper_terminalizes_stalled_task_with_dead_worker() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    store
        .insert_task(&task("t-stalled-dead", TaskStatus::Stalled))
        .expect("insert task");
    save_spec(&spec("t-stalled-dead")).expect("save spec");

    let cleaned = check_zombie_tasks_with(&store, |_| false).expect("reap");

    assert_eq!(cleaned, vec!["t-stalled-dead".to_string()]);
    assert_eq!(
        store.get_task("t-stalled-dead").expect("get").expect("task").status,
        TaskStatus::Failed
    );
}

#[test]
fn reaper_cleaned_list_excludes_tasks_that_did_not_transition() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    store
        .insert_task(&task("t-raced-done", TaskStatus::Running))
        .expect("insert task");
    save_spec(&spec("t-raced-done")).expect("save spec");

    let cleaned = check_zombie_tasks_with(&store, |pid| {
        if pid == 77 {
            store
                .update_task_status("t-raced-done", TaskStatus::Done)
                .expect("mark done");
        }
        false
    })
    .expect("reap");

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-raced-done").expect("get").expect("task").status,
        TaskStatus::Done
    );
}
