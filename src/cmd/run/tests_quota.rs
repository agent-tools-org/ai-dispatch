// Extracted quota tests from tests.rs.
// Deps: parent test fixtures and module imports.
use super::*;

#[test]
fn empty_diff_detection_respects_worktree_state() {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.email", "aid@example.com"]);
    git(dir.path(), &["config", "user.name", "Aid Tester"]);
    let file = dir.path().join("file.txt");
    std::fs::write(&file, "initial").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "initial"]);
    assert_eq!(worktree_is_empty_diff(dir.path()), Some(true));
    std::fs::write(&file, "updated").unwrap();
    assert_eq!(worktree_is_empty_diff(dir.path()), Some(false));
}

#[test]
fn take_next_cascade_agent_consumes_first_entry() {
    let args = RunArgs {
        agent_name: "primary".to_string(),
        cascade: vec!["codex".to_string(), "cursor".to_string()],
        ..Default::default()
    };
    let result = take_next_cascade_agent(&args);
    assert_eq!(result, Some(("codex".to_string(), vec!["cursor".to_string()])));
}

#[test]
fn take_next_cascade_agent_returns_none_when_empty() {
    let args = RunArgs { cascade: vec![], ..Default::default() };
    assert!(take_next_cascade_agent(&args).is_none());
}

#[test]
fn read_quota_error_message_uses_stderr() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    std::fs::write(
        paths::stderr_path("t-quota-stderr"),
        "You have hit your usage limit.",
    )
    .unwrap();
    let message = read_quota_error_message(&TaskId("t-quota-stderr".to_string()), &AgentKind::Codex);
    assert_eq!(message.as_deref(), Some("You have hit your usage limit."));
}

#[test]
fn read_quota_error_message_falls_back_to_log() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    std::fs::write(
        paths::log_path("t-quota-log"),
        "{\"error\":\"You have hit your usage limit.\"}\n",
    )
    .unwrap();
    let message = read_quota_error_message(&TaskId("t-quota-log".to_string()), &AgentKind::Codex);
    // The provider's sentence, lifted out of the envelope it arrived in — not
    // the raw JSON, which is what the marker used to end up holding.
    assert_eq!(message.as_deref(), Some("You have hit your usage limit."));
}

#[test]
fn read_quota_error_message_extracts_rate_limit_line_only() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    std::fs::write(
        paths::stderr_path("t-quota-mixed"),
        "tokens: 8714294 in + 27373 out = 8741667 (8442752 cached)\nYou have hit your usage limit.\nsome other line\n",
    )
    .unwrap();
    let message = read_quota_error_message(&TaskId("t-quota-mixed".to_string()), &AgentKind::Codex);
    assert_eq!(message.as_deref(), Some("You have hit your usage limit."));
}

#[test]
fn read_quota_error_message_detects_402_payment_errors() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    std::fs::write(
        paths::log_path("t-quota-402"),
        "{\"type\":\"error\",\"source\":\"agent_loop\",\"message\":\"402 payment required: reload your tokens\"}\n",
    )
    .unwrap();
    let message = read_quota_error_message(&TaskId("t-quota-402".to_string()), &AgentKind::Codex);
    assert_eq!(message.as_deref(), Some("402 payment required: reload your tokens"));
}

#[test]
fn read_quota_error_message_ignores_agent_grep_in_log() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    crate::rate_limit::clear_rate_limit(&AgentKind::Cursor, None);
    let grep_line = "completed: grep clear_rate_limit_if_stale|marker_path";
    std::fs::write(
        paths::log_path("t-quota-grep-log"),
        format!("{grep_line}\n"),
    )
    .unwrap();
    let task_id = TaskId("t-quota-grep-log".to_string());
    let message = read_quota_error_message(&task_id, &AgentKind::Cursor);
    assert_eq!(message, None);
    if let Some(line) = message.as_deref() {
        crate::rate_limit::mark_rate_limited(&AgentKind::Cursor, None, line);
    }
    assert!(!crate::rate_limit::is_rate_limited(&AgentKind::Cursor, None));
}

#[test]
fn rescue_quota_failed_task_refuses_empty_worktree_rescue() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();

    let wt_dir = dir.path().join("wt");
    std::fs::create_dir_all(&wt_dir).unwrap();
    git(&wt_dir, &["init"]);
    git(&wt_dir, &["config", "user.email", "aid@example.com"]);
    git(&wt_dir, &["config", "user.name", "Aid Tester"]);
    std::fs::write(wt_dir.join("file.txt"), "initial").unwrap();
    git(&wt_dir, &["add", "file.txt"]);
    git(&wt_dir, &["commit", "-m", "initial"]);

    std::fs::write(
        paths::stderr_path("t-empty-wt"),
        "Error: You have hit your usage limit.",
    )
    .unwrap();

    let store = Store::open_memory().unwrap();
    let mut task = make_failed_task("t-empty-wt");
    task.worktree_path = Some(wt_dir.to_str().unwrap().to_string());
    task.verify_status = VerifyStatus::Passed;
    store.insert_task(&task).unwrap();

    rescue_quota_failed_task(
        &store,
        &task.id,
        read_quota_error_message(&task.id, &task.agent).as_deref(),
    );

    let task = store.get_task("t-empty-wt").unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Failed);
}

#[test]
fn rescue_quota_failed_task_rescues_worktree_with_modified_code() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();

    let wt_dir = dir.path().join("wt");
    std::fs::create_dir_all(&wt_dir).unwrap();
    git(&wt_dir, &["init"]);
    git(&wt_dir, &["config", "user.email", "aid@example.com"]);
    git(&wt_dir, &["config", "user.name", "Aid Tester"]);
    std::fs::write(wt_dir.join("file.txt"), "initial").unwrap();
    git(&wt_dir, &["add", "file.txt"]);
    git(&wt_dir, &["commit", "-m", "initial"]);

    std::fs::write(
        paths::stderr_path("t-work-wt"),
        "Error: You have hit your usage limit.",
    )
    .unwrap();

    let store = Store::open_memory().unwrap();
    let mut task = make_failed_task("t-work-wt");
    task.worktree_path = Some(wt_dir.to_str().unwrap().to_string());
    task.verify_status = VerifyStatus::Passed;
    store.insert_task(&task).unwrap();

    // Clean worktree: guard must refuse rescue so task stays Failed.
    rescue_quota_failed_task(
        &store,
        &task.id,
        read_quota_error_message(&task.id, &task.agent).as_deref(),
    );
    let checked_task = store.get_task("t-work-wt").unwrap().unwrap();
    assert_eq!(checked_task.status, TaskStatus::Failed);

    // Modify file: guard allows rescue to Done.
    std::fs::write(wt_dir.join("file.txt"), "modified").unwrap();
    rescue_quota_failed_task(
        &store,
        &task.id,
        read_quota_error_message(&task.id, &task.agent).as_deref(),
    );

    let task = store.get_task("t-work-wt").unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Done);
}

#[test]
fn rescue_quota_failed_task_rescues_untracked_source_files() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();

    let wt_dir = dir.path().join("wt");
    std::fs::create_dir_all(&wt_dir).unwrap();
    git(&wt_dir, &["init"]);
    git(&wt_dir, &["config", "user.email", "aid@example.com"]);
    git(&wt_dir, &["config", "user.name", "Aid Tester"]);
    std::fs::write(wt_dir.join("file.txt"), "initial").unwrap();
    git(&wt_dir, &["add", "file.txt"]);
    git(&wt_dir, &["commit", "-m", "initial"]);

    std::fs::write(
        paths::stderr_path("t-untracked-wt"),
        "Error: You have hit your usage limit.",
    )
    .unwrap();

    let store = Store::open_memory().unwrap();
    let mut task = make_failed_task("t-untracked-wt");
    task.worktree_path = Some(wt_dir.to_str().unwrap().to_string());
    task.verify_status = VerifyStatus::Passed;
    store.insert_task(&task).unwrap();

    // Nothing written yet, so the guard must refuse. Without this half the test
    // passes even when `produced_work` is forced to return true — a cross-audit
    // caught it doing exactly that, and a mutation run confirmed it.
    rescue_quota_failed_task(
        &store,
        &task.id,
        read_quota_error_message(&task.id, &task.agent).as_deref(),
    );
    let checked = store.get_task("t-untracked-wt").unwrap().unwrap();
    assert_eq!(checked.status, TaskStatus::Failed);

    // The agent's only output is an untracked file. `git diff` cannot see it,
    // which is how the first version of this guard threw such work away.
    std::fs::write(wt_dir.join("new_file.txt"), "untracked work").unwrap();

    rescue_quota_failed_task(
        &store,
        &task.id,
        read_quota_error_message(&task.id, &task.agent).as_deref(),
    );

    let task = store.get_task("t-untracked-wt").unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Done);
}
