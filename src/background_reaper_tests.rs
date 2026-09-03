// Regression tests for background reaper terminalization behavior.
// Covers active execution failures and cleaned-list bookkeeping.

use chrono::{Duration, Local};
use std::fs;

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
        result_file_required: None,
        model: None,
        budget: false,
        session_id: None,
        verify: None,
        setup: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        judge_retry: false,
        max_duration_mins: Some(60),
        max_duration_secs: None,
        max_task_cost: None,
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
        audit_explicit: false,
        no_audit: false,
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
        foreground: false,
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

/// A missing worker always fails the task and kills any recorded agent.
#[test]
fn reaps_task_with_dead_worker_and_kills_its_live_agent() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    store
        .insert_task(&task("t-orphan-live", TaskStatus::Running))
        .expect("insert task");
    let mut child = std::process::Command::new("sleep").arg("30").spawn().expect("spawn agent");
    let agent_pid = child.id();
    let mut s = spec("t-orphan-live");
    s.agent_pid = Some(agent_pid);
    s.idle_timeout_secs = Some(3600);
    save_spec(&s).expect("save spec");

    let cleaned = check_zombie_tasks_with(&store, |_| false).expect("reap");

    assert_eq!(cleaned, vec!["t-orphan-live".to_string()]);
    assert_eq!(
        store.get_task("t-orphan-live").expect("get").expect("task").status,
        TaskStatus::Failed,
    );
    let _ = child.wait();
    assert!(
        !crate::background::is_process_running(agent_pid),
        "an orphan agent with no detach marker must still be killed",
    );
}

#[test]
fn reaps_task_with_dead_worker_and_agent() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    store
        .insert_task(&task("t-no-detach", TaskStatus::Running))
        .expect("insert task");
    let mut s = spec("t-no-detach");
    s.agent_pid = Some(999999);
    s.idle_timeout_secs = Some(3600);
    save_spec(&s).expect("save spec");

    let cleaned = check_zombie_tasks_with(&store, |_| false).expect("reap");

    assert_eq!(cleaned, vec!["t-no-detach".to_string()]);
    assert_eq!(
        store.get_task("t-no-detach").expect("get").expect("task").status,
        TaskStatus::Failed,
    );
}

#[test]
fn reaper_skips_unreadable_spec_and_continues_with_other_tasks() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    store
        .insert_task(&task("t-unreadable-spec", TaskStatus::Running))
        .expect("insert unreadable task");
    store
        .insert_task(&task("t-valid-after-unreadable", TaskStatus::Running))
        .expect("insert valid task");
    fs::write(
        paths::job_path("t-unreadable-spec"),
        "{ not valid background spec",
    )
    .expect("write malformed spec");
    save_spec(&spec("t-valid-after-unreadable")).expect("save valid spec");

    let cleaned = check_zombie_tasks_with(&store, |_| false).expect("reap");

    assert_eq!(cleaned, vec!["t-valid-after-unreadable".to_string()]);
    assert_eq!(
        store
            .get_task("t-unreadable-spec")
            .expect("get unreadable")
            .expect("unreadable task")
            .status,
        TaskStatus::Running
    );
    assert_eq!(
        store
            .get_task("t-valid-after-unreadable")
            .expect("get valid")
            .expect("valid task")
            .status,
        TaskStatus::Failed
    );
}
