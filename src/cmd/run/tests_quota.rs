// Extracted quota tests from tests.rs.
// Deps: parent test fixtures and module imports.
use super::*;
use crate::routing::rate_limit;

#[test]
fn failed_pty_quota_excerpt_allows_continuation_without_promoting_its_hold() {
    let _home = crate::support::test_env::TestHome::new();
    paths::ensure_dirs().expect("directories");
    let mut task = make_failed_task("t-oz-pty-quota");
    task.agent = AgentKind::Oz;
    let message = "Error: Quota limit reached.";
    std::fs::write(paths::log_path(task.id.as_str()), message).expect("PTY log");
    assert_eq!(read_quota_error_message(&task.id, &task.agent), None);
    assert_eq!(
        run_post::record_failed_quota(&task, task.agent).as_deref(),
        Some(message)
    );
    let marker = std::fs::read_to_string(rate_limit::marker_path(&task.agent, None))
        .expect("transient marker");
    assert_eq!(
        crate::routing::availability::stored_hold(&marker, &task.agent),
        crate::routing::availability::StoredHold::Transient
    );
    task.status = TaskStatus::Done;
    assert_eq!(run_post::record_failed_quota(&task, task.agent), None);
    task.status = TaskStatus::Failed;
    for text in [
        "You have exceeded your monthly quota",
        "429 Too Many Requests",
        r#"{"type":"agent","text":"Error: Quota limit reached."}"#,
    ] {
        std::fs::write(paths::log_path(task.id.as_str()), text).expect("log");
        assert_eq!(run_post::record_failed_quota(&task, task.agent), None);
    }
    std::fs::write(paths::stderr_path(task.id.as_str()), message).expect("stderr");
    assert_eq!(
        run_post::record_failed_quota(&task, task.agent).as_deref(),
        Some(message)
    );
    let marker = std::fs::read_to_string(rate_limit::marker_path(&task.agent, None))
        .expect("attributed marker");
    assert!(matches!(
        crate::routing::availability::stored_hold(&marker, &task.agent),
        crate::routing::availability::StoredHold::Until(_)
    ));
}

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
    let _dir = crate::support::test_env::TestHome::new();
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
    let _dir = crate::support::test_env::TestHome::new();
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
    let _dir = crate::support::test_env::TestHome::new();
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
    let _dir = crate::support::test_env::TestHome::new();
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
    let _dir = crate::support::test_env::TestHome::new();
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    crate::routing::rate_limit::clear_rate_limit(&AgentKind::Cursor, None);
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
        crate::routing::rate_limit::mark_rate_limited(&AgentKind::Cursor, None, line);
    }
    assert!(!crate::routing::rate_limit::is_rate_limited(&AgentKind::Cursor, None));
}

#[test]
fn terminal_failure_refuses_empty_worktree_rescue() {
    let dir = crate::support::test_env::TestHome::new();
    std::fs::create_dir_all(paths::logs_dir()).unwrap();

    let wt_dir = quota_worktree(dir.path(), None);

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

    assert_terminal_failure_preserved(
        &store,
        &task.id,
        read_quota_error_message(&task.id, &task.agent).as_deref(),
    );

    let task = store.get_task("t-empty-wt").unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Failed);
}

#[test]
fn terminal_failure_preserves_worktree_changes() {
    for (case, task_id, changed_file, contents) in [
        ("terminal_failure_preserves_worktree_with_modified_code", "t-work-wt", "file.txt", "modified"),
        (
            "terminal_failure_preserves_untracked_source_files",
            "t-untracked-wt",
            "new_file.txt",
            "untracked work",
        ),
    ] {
        eprintln!("case: {case}");
        let dir = crate::support::test_env::TestHome::new();
        std::fs::create_dir_all(paths::logs_dir()).unwrap();

        let wt_dir = quota_worktree(dir.path(), None);

        std::fs::write(
            paths::stderr_path(task_id),
            "Error: You have hit your usage limit.",
        )
        .unwrap();

        let store = Store::open_memory().unwrap();
        let mut task = make_failed_task(task_id);
        task.worktree_path = Some(wt_dir.to_str().unwrap().to_string());
        task.verify_status = VerifyStatus::Passed;
        store.insert_task(&task).unwrap();

        // The terminal failure is preserved with a clean worktree.
        assert_terminal_failure_preserved(
            &store,
            &task.id,
            read_quota_error_message(&task.id, &task.agent).as_deref(),
        );
        let checked_task = store.get_task(task_id).unwrap().unwrap();
        assert_eq!(checked_task.status, TaskStatus::Failed);

        // Modifying a file does not reopen the terminal result.
        std::fs::write(wt_dir.join(changed_file), contents).unwrap();
        assert_terminal_failure_preserved(
            &store,
            &task.id,
            read_quota_error_message(&task.id, &task.agent).as_deref(),
        );

        let task = store.get_task(task_id).unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(std::fs::read_to_string(wt_dir.join(changed_file)).unwrap(), contents);
    }
}
