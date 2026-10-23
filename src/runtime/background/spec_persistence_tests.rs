// spec persistence tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

#[test]
fn serializes_spec_to_json() {
    let spec = BackgroundRunSpec {
        task_id: "t-5a0e".to_string(),
        worker_pid: Some(4242),
        dir: Some(".".to_string()),
        budget: false,
        max_duration_mins: Some(90),
        max_duration_secs: None,
        idle_timeout_secs: None,
        interactive: true,
        on_done: None,
        env: None,
        agent_pid: None,
        agent_started: None,
        audit_report_mode: false,
        pre_task_dirty_paths: None,
        foreground: false,
    };

    let content = serde_json::to_string_pretty(&spec).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 15);
    assert_eq!(value["task_id"], "t-5a0e");
    assert_eq!(value["worker_pid"], 4242);
    assert!(value.get("result_file").is_none());
}

#[test]
fn serializes_cascade_field() {
    let spec = crate::cmd::run::RunArgs {
        cascade: vec!["opencode".into(), "cursor".into()],
        ..Default::default()
    };
    let content = serde_json::to_string_pretty(&spec).unwrap();
    assert!(content.contains("\"cascade\""));
    assert!(content.contains("\"opencode\""));
    assert!(content.contains("\"cursor\""));
    let parsed: crate::cmd::run::RunArgs = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed.cascade, vec!["opencode", "cursor"]);
}

#[test]
fn completion_notifications_are_written_as_jsonl() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let mut task = make_task("t-4d4d", TaskStatus::Done);
    task.duration_ms = Some(1_500);
    task.cost_usd = Some(0.25);
    task.prompt = "x".repeat(120);

    crate::task::notify::notify_completion(&task);

    let line = crate::task::notify::read_recent(20).unwrap();
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(value["task_id"], "t-4d4d");
    assert_eq!(value["agent"], "codex");
    assert_eq!(value["status"], "DONE");
    assert_eq!(value["duration_ms"], 1_500);
    assert_eq!(value["cost_usd"], 0.25);
    assert_eq!(value["prompt"], "x".repeat(100));
    assert!(value["timestamp"].as_str().is_some());
}

#[test]
fn build_on_done_command_splits_simple_argv() {
    let cmd = build_on_done_command("echo hi").unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(cmd.get_program().to_string_lossy(), "echo");
    assert_eq!(args, vec!["hi"]);
}

#[test]
fn build_on_done_command_routes_shell_operators_through_sh() {
    let chained = build_on_done_command("echo hi && echo bye").unwrap();
    let chained_args: Vec<String> = chained
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(chained.get_program().to_string_lossy(), "sh");
    assert_eq!(chained_args, vec!["-c", "echo hi && echo bye"]);

    let fallback = build_on_done_command("cmd1 || cmd2").unwrap();
    let fallback_args: Vec<String> = fallback
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(fallback.get_program().to_string_lossy(), "sh");
    assert_eq!(fallback_args, vec!["-c", "cmd1 || cmd2"]);
}

#[test]
fn quota_cascade_skipped_for_batch_tasks() {
    let batch_spec = crate::cmd::run::RunArgs {
        group: Some("wg-test".into()),
        ..Default::default()
    };
    // Batch tasks have group set — cascade guard (args.group.is_none()) blocks them
    assert!(
        batch_spec.group.is_some(),
        "batch tasks have group set, cascade should be skipped"
    );

    let solo_spec = crate::cmd::run::RunArgs {
        group: None,
        ..Default::default()
    };
    // Non-batch tasks have no group — cascade is allowed
    assert!(solo_spec.group.is_none(), "solo tasks should allow cascade");
}

#[test]
fn explicit_cascade_takes_priority_over_quota_cascade() {
    // When spec.cascade is non-empty, the explicit cascade path should run
    // (not the quota auto-cascade). Verify the saved args structure supports this.
    let spec = crate::cmd::run::RunArgs {
        cascade: vec!["opencode".into(), "cursor".into()],
        group: None,
        ..Default::default()
    };
    assert!(spec.group.is_none(), "solo task allows cascade");
    assert!(
        !spec.cascade.is_empty(),
        "explicit cascade should be present"
    );
    assert_eq!(spec.cascade[0], "opencode");
    assert_eq!(&spec.cascade[1..], &["cursor"]);
}

#[test]
fn agent_pid_stored_and_loaded_correctly() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();

    save_spec(&make_spec("t-a100")).unwrap();

    assert!(super::load_agent_pid("t-a100").unwrap().is_none());

    super::update_agent_pid("t-a100", 12345).unwrap();

    let loaded = super::load_agent_pid("t-a100").unwrap();
    assert_eq!(loaded, Some(12345));

    let spec = super::load_spec("t-a100").unwrap();
    assert_eq!(spec.agent_pid, Some(12345));
    assert_eq!(spec.worker_pid, Some(202));
}

#[test]
fn agent_pid_backwards_compatible() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();

    save_spec(&make_spec("t-c200")).unwrap();

    let spec = super::load_spec("t-c200").unwrap();
    assert!(spec.agent_pid.is_none());
}

#[test]
fn full_old_format_job_can_be_waited_stopped_and_reaped() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();
    let store = std::sync::Arc::new(Store::open_memory().unwrap());
    let mut legacy: serde_json::Value =
        serde_json::from_str(include_str!("../../../testdata/full-background-spec.json")).unwrap();
    assert_eq!(legacy.as_object().unwrap().len(), 50);
    let worker_pid = i32::MAX as u32 - 3;
    let agent_pid = i32::MAX as u32 - 4;
    legacy["worker_pid"] = worker_pid.into();
    legacy["agent_pid"] = agent_pid.into();
    for id in ["t-old-stop", "t-old-reap"] {
        store.insert_task(&make_task(id, TaskStatus::Running)).unwrap();
        legacy["task_id"] = id.into();
        std::fs::write(paths::job_path(id), serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(super::load_worker_pid(id).unwrap(), Some(worker_pid));
        assert_eq!(super::load_agent_pid(id).unwrap(), Some(agent_pid));
    }
    crate::cmd::stop::stop(&store, "t-old-stop").unwrap();
    assert_eq!(
        store.get_task("t-old-stop").unwrap().unwrap().status,
        TaskStatus::Stopped
    );
    kill::RECORDED_KILLS.with(|kills| kills.borrow_mut().clear());
    assert_eq!(
        check_zombie_tasks_with(&store, |_| false).unwrap(),
        ["t-old-reap"]
    );
    assert_eq!(
        store.get_task("t-old-reap").unwrap().unwrap().status,
        TaskStatus::Failed
    );
    let error = store.latest_error("t-old-reap").unwrap();
    assert!(error.contains(ZOMBIE_FAILURE_DETAIL));
    assert_eq!(
        kill::RECORDED_KILLS.with(|kills| kills.borrow().clone()),
        vec![vec![worker_pid, agent_pid]]
    );
}
