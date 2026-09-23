// Regression coverage for task waiting and verification.
use super::*;
use chrono::Local;
use crate::types::{AgentKind, EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus};

pub(super) fn make_task(id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "test prompt".to_string(),
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

#[tokio::test]
async fn wait_for_task_ids_times_out_with_running_tasks() {
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-run", TaskStatus::Running)).unwrap();
    let outcome = wait_for_task_ids(&store, &[String::from("t-run")], None, false, Some(Duration::from_millis(10)))
        .await
        .unwrap();
    assert_eq!(outcome, WaitOutcome::TimedOut(vec![String::from("t-run")]));
}

#[tokio::test]
async fn wait_for_task_ids_completes_with_existing_milestone_event() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-done", TaskStatus::Done);
    task.duration_ms = Some(1_000);
    store.insert_task(&task).unwrap();
    store.insert_event(&TaskEvent {
        task_id: TaskId("t-done".to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Milestone,
        detail: "background progress".to_string(),
        metadata: None,
    }).unwrap();
    let outcome = wait_for_task_ids(&store, &[String::from("t-done")], None, false, None).await.unwrap();
    assert_eq!(outcome, WaitOutcome::Completed);
}

#[tokio::test]
async fn wait_for_task_ids_reports_failed_terminal_tasks() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = make_task("t-failed", TaskStatus::Failed);
    store.insert_task(&task).unwrap();

    let outcome = wait_for_task_ids(&store, &[String::from("t-failed")], None, false, None)
        .await
        .unwrap();

    assert_eq!(outcome, WaitOutcome::Failed(vec![String::from("t-failed")]));
}

#[tokio::test]
async fn wait_for_task_ids_times_out_when_pending_verification_never_answers() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-no-verify-result", TaskStatus::Done);
    task.verify = Some("cargo test".to_string());
    task.verify_status = VerifyStatus::Pending;
    store.insert_task(&task).unwrap();

    let outcome = wait_for_task_ids(
        &store,
        &[String::from("t-no-verify-result")],
        None,
        false,
        Some(Duration::from_millis(10)),
    )
    .await
    .unwrap();

    assert_eq!(
        outcome,
        WaitOutcome::TimedOut(vec![String::from("t-no-verify-result")])
    );
}

#[tokio::test]
async fn wait_for_task_ids_waits_for_done_task_verification_result() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-verifying", TaskStatus::Done);
    task.verify = Some("cargo test".to_string());
    task.verify_status = VerifyStatus::Pending;
    store.insert_task(&task).unwrap();

    let wait_store = store.clone();
    let handle = tokio::spawn(async move {
        wait_for_task_ids(&wait_store, &[String::from("t-verifying")], None, false, None).await
    });

    sleep(Duration::from_millis(100)).await;
    assert!(!handle.is_finished());
    store
        .update_verify_status("t-verifying", VerifyStatus::Passed)
        .unwrap();

    let outcome = tokio::time::timeout(Duration::from_secs(3), handle)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(outcome, WaitOutcome::Completed);
}

#[tokio::test]
async fn wait_for_task_ids_tracks_stalled_tasks_until_they_finish() {
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .insert_task(&make_task("t-stalled", TaskStatus::Stalled))
        .unwrap();
    let wait_store = store.clone();
    let handle = tokio::spawn(async move {
        wait_for_task_ids(&wait_store, &[String::from("t-stalled")], None, false, None).await
    });

    sleep(Duration::from_millis(100)).await;
    assert!(!handle.is_finished());
    store.update_task_status("t-stalled", TaskStatus::Done).unwrap();

    let outcome = tokio::time::timeout(Duration::from_secs(3), handle)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(outcome, WaitOutcome::Completed);
}

#[tokio::test]
async fn wait_for_task_ids_tracks_group_tasks_added_mid_watch() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut first = make_task("t-first", TaskStatus::Running);
    first.workgroup_id = Some("wg-dyn".to_string());
    store.insert_task(&first).unwrap();
    let wait_store = store.clone();
    let handle = tokio::spawn(async move { wait_for_task_ids(&wait_store, &[String::from("t-first")], Some("wg-dyn"), false, None).await });
    sleep(Duration::from_millis(100)).await;
    let mut second = make_task("t-second", TaskStatus::Running);
    second.workgroup_id = Some("wg-dyn".to_string());
    store.insert_task(&second).unwrap();
    store.update_task_status("t-first", TaskStatus::Done).unwrap();
    sleep(Duration::from_millis(2_100)).await;
    assert!(!handle.is_finished());
    store.update_task_status("t-second", TaskStatus::Done).unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(3), handle).await.unwrap().unwrap().unwrap();
    assert_eq!(outcome, WaitOutcome::Completed);
}

#[tokio::test]
async fn wait_for_task_ids_tracks_pending_group_tasks_added_mid_watch() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut first = make_task("t-first", TaskStatus::Running);
    first.workgroup_id = Some("wg-dyn".to_string());
    store.insert_task(&first).unwrap();
    let wait_store = store.clone();
    let handle = tokio::spawn(async move {
        wait_for_task_ids(&wait_store, &[String::from("t-first")], Some("wg-dyn"), false, None).await
    });
    sleep(Duration::from_millis(100)).await;
    let mut second = make_task("t-second", TaskStatus::Pending);
    second.workgroup_id = Some("wg-dyn".to_string());
    store.insert_task(&second).unwrap();
    store.update_task_status("t-first", TaskStatus::Done).unwrap();
    sleep(Duration::from_millis(2_100)).await;
    assert!(!handle.is_finished());
    store.update_task_status("t-second", TaskStatus::Running).unwrap();
    sleep(Duration::from_millis(100)).await;
    assert!(!handle.is_finished());
    store.update_task_status("t-second", TaskStatus::Done).unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(3), handle).await.unwrap().unwrap().unwrap();
    assert_eq!(outcome, WaitOutcome::Completed);
}
