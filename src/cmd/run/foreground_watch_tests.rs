// Tests for foreground attachment completion and retry-chain following.
// Covers the real Store polling boundary used after background dispatch.
// Deps: run_foreground_watch, Store, task domain types, and Tokio.

use super::*;
use chrono::Local;
use crate::background::{self, BackgroundRunSpec};
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use std::fs;
use std::sync::Arc;
use std::time::Duration;

fn task(id: &str, status: TaskStatus, parent: Option<&str>) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "real task watcher test".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: parent.map(ToString::to_string),
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: Some(1_000),
        requested_model: None,
        observed_model: None,
        attribution_source: None,
        cost_usd: None,
        exit_code: Some(0),
        created_at: Local::now(),
        completed_at: Some(Local::now()),
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
fn task_completion_requires_verification_to_settle() {
    let mut entry = task("t-watch-verify", TaskStatus::Done, None);
    entry.verify = Some("aid build".to_string());
    entry.verify_status = VerifyStatus::Pending;
    assert!(!task_is_complete(&entry));
    entry.verify_status = VerifyStatus::Passed;
    assert!(task_is_complete(&entry));
}

#[tokio::test]
async fn foreground_watcher_follows_a_retry_created_after_the_original_task() {
    let store = Arc::new(Store::open_memory().expect("store"));
    store
        .insert_task(&task("t-watch-root", TaskStatus::Done, None))
        .expect("root task");
    store
        .insert_task(&task(
            "t-watch-retry",
            TaskStatus::Running,
            Some("t-watch-root"),
        ))
        .expect("retry task");

    let update_store = Arc::clone(&store);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        update_store
            .update_task_status("t-watch-retry", TaskStatus::Done)
            .expect("complete retry");
    });

    let final_id = wait_for_task(&store, &TaskId("t-watch-root".to_string()))
        .await
        .expect("watcher result");
    assert_eq!(final_id.as_str(), "t-watch-retry");
}

#[tokio::test]
async fn foreground_watcher_observes_a_real_background_worker() {
    let home = tempfile::tempdir().expect("home");
    let _aid_home = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("aid dirs");
    let agents_dir = paths::aid_dir().join("agents");
    fs::create_dir_all(&agents_dir).expect("agents dir");
    fs::write(
        agents_dir.join("watch-test.toml"),
        r#"[agent]
id = "watch-test"
display_name = "Watcher Test"
command = "/bin/sh"
prompt_mode = "arg"
fixed_args = ["-c", "printf 'real worker progress\\n'"]
interactive_input = false
"#,
    )
    .expect("agent config");

    let store = Arc::new(Store::open_memory().expect("store"));
    let mut real_task = task("t-watch-real", TaskStatus::Running, None);
    real_task.read_only = true;
    store
        .insert_task(&real_task)
        .expect("task");
    let spec: BackgroundRunSpec = serde_json::from_value(serde_json::json!({
        "task_id": "t-watch-real",
        "worker_pid": null,
        "agent_name": "watch-test",
        "prompt": "run the real worker",
        "dir": ".",
        "output": null,
        "model": null,
        "verify": null,
        "retry": 0,
        "group": null,
        "interactive": true,
        "read_only": true
    }))
    .expect("spec");
    background::save_spec(&spec).expect("save spec");

    let worker_store = Arc::clone(&store);
    let worker = tokio::spawn(async move {
        background::run_task(worker_store, "t-watch-real").await
    });
    let final_id = wait_for_task(&store, &TaskId("t-watch-real".to_string()))
        .await
        .expect("watcher result");
    worker.await.expect("worker join").expect("worker result");

    assert_eq!(final_id.as_str(), "t-watch-real");
    assert_eq!(store.get_task("t-watch-real").expect("get").expect("task").status, TaskStatus::Done);
    assert!(
        fs::read_to_string(paths::log_path("t-watch-real"))
            .expect("worker log")
            .contains("real worker progress")
    );
}
