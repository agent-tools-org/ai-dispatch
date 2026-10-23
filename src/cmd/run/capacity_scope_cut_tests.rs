// Capacity failures use only the caller's ordinary retry budget.
// Exercises decoded evidence, completion reasons, holds and real retry dispatch.

use super::*;

#[tokio::test]
async fn provider_capacity_scope_cut_retry_zero_creates_no_child() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().expect("store"));
    let row = task("t-capacity-zero", home.path());
    store.insert_task(&row).expect("parent");
    seed_stdout(
        &store,
        &row.id,
        &serde_json::json!({"type": "error", "message": CAPACITY}).to_string(),
    );
    let args = RunArgs {
        retry: 0,
        dir: row.effective_dir.clone(),
        ..Default::default()
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        prepare_retry(store.clone(), &row.id, &args),
    )
    .await;
    assert_eq!(
        store.get_retry_tree(row.id.as_str()).expect("tree").len(),
        1
    );
    assert!(
        result
            .expect("no capacity backoff")
            .expect("retry")
            .is_none()
    );
    assert_eq!(
        store
            .get_task(row.id.as_str())
            .expect("lookup")
            .expect("row")
            .status,
        TaskStatus::Failed
    );
}

#[tokio::test]
async fn provider_capacity_scope_cut_retry_one_runs_ordinary_retry_once() {
    let _permit = crate::support::test_subprocess::acquire();
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let (store, row, args, marker) = ordinary_retry_fixture(home.path());
    let child = crate::cmd::run::run_lifecycle::retry_or_cascade(&store, &args, &row.id)
        .await
        .expect("ordinary retry")
        .expect("child");
    let saved = RunArgs::saved_for_task(&store, child.as_str())
        .expect("saved")
        .expect("args");
    assert_eq!(
        saved.retry, 0,
        "ordinary retry consumes the caller's budget"
    );
    assert!(saved.prompt.starts_with("[Previous attempt failed]"));
    assert_eq!(
        store.get_retry_tree(row.id.as_str()).expect("tree").len(),
        2
    );
    assert_eq!(
        store
            .get_task(child.as_str())
            .expect("lookup")
            .expect("row")
            .status,
        TaskStatus::Done
    );
    assert_eq!(
        store
            .get_task(child.as_str())
            .expect("lookup")
            .expect("row")
            .verify_status,
        VerifyStatus::Passed
    );
    assert_eq!(
        std::fs::read_to_string(marker).expect("launches"),
        "launch\n"
    );
}

#[tokio::test]
async fn provider_capacity_scope_cut_decoded_failure_records_model_reason_and_hold() {
    let _permit = crate::support::test_subprocess::acquire();
    for envelope in [
        serde_json::json!({"type": "error", "message": CAPACITY}),
        serde_json::json!({"type": "turn.failed", "error": {"message": CAPACITY}}),
    ] {
        let home = tempfile::tempdir().expect("home");
        let _home = paths::AidHomeGuard::set(home.path());
        paths::ensure_dirs().expect("dirs");
        let store = Arc::new(Store::open_memory().expect("store"));
        let mut row = task("t-capacity-decoded-reason", home.path());
        row.status = TaskStatus::Running;
        store.insert_task(&row).expect("parent");
        let mut cmd = tokio::process::Command::new("/bin/sh");
        cmd.args(["-c", &format!("echo '{envelope}'; exit 1")]);
        crate::cmd::run::run_agent::run_agent_process_with_cost(
            &crate::agent::codex::CodexAgent,
            cmd,
            &row.id,
            &store,
            &paths::log_path(row.id.as_str()),
            None,
            row.requested_model.as_deref(),
            true,
            None,
            Default::default(),
            None,
            std::time::Instant::now(),
        )
        .await
        .expect("process failure");
        assert_capacity_failure(&store, &row);
    }
}

fn ordinary_retry_fixture(
    home: &std::path::Path,
) -> (Arc<Store>, Task, RunArgs, std::path::PathBuf) {
    paths::ensure_dirs().expect("dirs");
    let marker = home.join("launches");
    install_agent(
        home,
        "capacity-budget",
        &format!("echo launch >> '{}'; echo completed", marker.display()),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut row = task("t-capacity-budget", home);
    row.agent = AgentKind::Custom;
    row.custom_agent_name = Some("capacity-budget".into());
    row.verify = Some("true".into());
    store.insert_task(&row).expect("parent");
    seed_stdout(
        &store,
        &row.id,
        &serde_json::json!({"type": "turn.failed", "error": {"message": CAPACITY}}).to_string(),
    );
    let args = RunArgs {
        agent_name: "capacity-budget".into(),
        dir: row.effective_dir.clone(),
        retry: 1,
        foreground: true,
        no_audit: true,
        verify: row.verify.clone(),
        skills: vec![crate::cmd::run::NO_SKILL_SENTINEL.into()],
        ..Default::default()
    };
    store
        .update_task_dispatch_args(row.id.as_str(), &args.dispatch_args_json().expect("json"))
        .expect("saved args");
    (store, row, args, marker)
}

fn assert_capacity_failure(store: &Store, row: &Task) {
    let failed = store
        .get_task(row.id.as_str())
        .expect("lookup")
        .expect("row");
    assert_eq!(failed.status, TaskStatus::Failed);
    let reason = store.latest_error(row.id.as_str()).expect("reason");
    assert!(reason.contains(CAPACITY));
    assert!(reason.contains("gpt-6.1-sol"), "reason must name the model");
    let hold = rate_limit::dispatch_blocking_hold_for_model(
        &row.agent,
        None,
        row.requested_model.as_deref(),
    )
    .expect("hold");
    assert!(hold.contains("provider capacity"));
    let marker = std::fs::read_to_string(rate_limit::group_marker_path(
        &row.agent,
        None,
        &provider_capacity::group(row.requested_model.as_deref()),
    ))
    .expect("capacity marker");
    assert!(marker.contains(CAPACITY));
    let events = store.get_events(row.id.as_str()).expect("events");
    let rendered = crate::ui::board::render_task_detail(&failed, &events, None);
    assert!(rendered.contains("Status: provider capacity"));
}

#[test]
fn provider_capacity_scope_cut_pty_merged_stderr_records_capacity_without_child() {
    let _permit = crate::support::test_subprocess::acquire();
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut row = task("t-capacity-pty", home.path());
    row.status = TaskStatus::Running;
    store.insert_task(&row).expect("parent");
    let envelope = serde_json::json!({"type": "error", "message": CAPACITY});
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", &format!("echo '{envelope}' >&2; exit 1")]);
    crate::runtime::pty::runner::run_agent_process_with_control(
        &crate::agent::codex::CodexAgent,
        &cmd,
        &row.id,
        &store,
        &paths::log_path(row.id.as_str()),
        None,
        row.requested_model.as_deref(),
        true,
        crate::config::timeout_policy::TimeoutPolicy::from_command(&cmd),
        None,
        None,
        std::time::Instant::now(),
    )
    .expect("PTY failure");
    assert_capacity_failure(&store, &row);
    assert_eq!(
        store.get_retry_tree(row.id.as_str()).expect("tree").len(),
        1
    );
}
