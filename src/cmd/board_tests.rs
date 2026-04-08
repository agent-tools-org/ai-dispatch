// Tests for `cmd::board` anti-poll behavior and board output helpers.
// Covers limit handling, marker transitions, and rendered output.
// Deps: super module internals, tempfile, chrono, crate::store, crate::paths.

use super::{AntiPollStatus, TruncationNotice, anti_poll_status, apply_limit, board_json_row, long_running_warning, truncation_notice_message, write_board_marker, write_board_output};
use chrono::{Duration, Local};
use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};

#[test]
fn long_running_warning_counts_running_tasks_older_than_one_hour() {
    let now = Local::now();
    let tasks = vec![
        make_task("t-1001", TaskStatus::Running, now - Duration::hours(1)),
        make_task("t-1002", TaskStatus::Running, now - Duration::minutes(59)),
        make_task("t-1003", TaskStatus::Done, now - Duration::hours(3)),
    ];
    let warning = long_running_warning(&tasks, now).unwrap();
    assert!(warning.contains("1 task(s) running >1h"));
}

#[test]
fn board_with_limit_truncates_output() {
    let now = Local::now();
    let mut tasks = vec![
        make_task("t-1001", TaskStatus::Done, now),
        make_task("t-1002", TaskStatus::Done, now),
        make_task("t-1003", TaskStatus::Done, now),
    ];
    let truncation = apply_limit(&mut tasks, Some(2), false, false, false, None);
    assert_eq!(tasks.len(), 2);
    assert_eq!(truncation, Some(TruncationNotice { shown: 2, total: 3 }));
    assert_eq!(
        truncation_notice_message(truncation.unwrap()),
        "[aid] Showing 2 of 3 tasks. Use --limit N or --today/--running for more."
    );
}

#[test]
fn board_json_row_includes_pending_reason() {
    let mut task = make_task("t-1004", TaskStatus::Failed, Local::now());
    task.pending_reason = Some("worker_capacity".to_string());
    let row = board_json_row(&task);
    assert_eq!(row["pending_reason"], "worker_capacity");
}

#[test]
fn format_group_header_includes_custom_name() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    let workgroup = store.create_workgroup("My Batch", "", Some("seed"), Some("wg-batch")).unwrap();
    assert_eq!(super::format_group_header(&workgroup), "Workgroup: wg-batch (My Batch)\n\n");
}

#[test]
fn board_output_is_written_before_anti_poll_exit() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let marker = crate::paths::aid_dir().join("board-last.txt");
    std::fs::write(&marker, "100\nfp\n0").unwrap();
    let store = Store::open_memory().unwrap();
    let tasks = vec![make_task("t-1001", TaskStatus::Done, Local::now())];
    let mut output = Vec::new();

    write_board_output(&mut output, &store, &tasks, None, None, false).unwrap();

    let rendered = String::from_utf8(output).unwrap();
    assert!(rendered.contains("t-1001"));
    assert_eq!(anti_poll_status(&marker, "changed", 103, false).0, AntiPollStatus::Cooldown(3));
}

#[test]
fn test_anti_poll_cooldown_blocks_rapid_calls() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let marker = crate::paths::aid_dir().join("board-last.txt");
    std::fs::write(&marker, "100\nfp\n0").unwrap();
    assert_eq!(anti_poll_status(&marker, "changed", 103, false).0, AntiPollStatus::Cooldown(3))
}

#[test]
fn test_anti_poll_force_bypasses_cooldown() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let marker = crate::paths::aid_dir().join("board-last.txt");
    std::fs::write(&marker, "100\nfp\n0").unwrap();
    assert_eq!(anti_poll_status(&marker, "changed", 103, true).0, AntiPollStatus::ForceCooldown(3))
}

#[test]
fn test_force_cooldown_blocks_within_30s() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let marker = crate::paths::aid_dir().join("board-last.txt");
    write_board_marker(&marker, "fp", 100, 0, 0, 0);
    assert_eq!(anti_poll_status(&marker, "changed", 120, true).0, AntiPollStatus::ForceCooldown(20));
}

#[test]
fn test_force_cooldown_allows_after_30s() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let marker = crate::paths::aid_dir().join("board-last.txt");
    write_board_marker(&marker, "fp", 100, 0, 0, 0);
    assert_eq!(anti_poll_status(&marker, "changed", 130, true).0, AntiPollStatus::Allowed(0));
}

#[test]
fn test_force_escalation_blocks_after_3_calls() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let marker = crate::paths::aid_dir().join("board-last.txt");

    write_board_marker(&marker, "fp", 100, 0, 1, 100);
    let (_, force_state) = anti_poll_status(&marker, "changed", 131, true);
    write_board_marker(&marker, "changed", 131, 0, force_state.count, force_state.window_start);

    let (_, force_state) = anti_poll_status(&marker, "changed", 162, true);
    write_board_marker(&marker, "changed", 162, 0, force_state.count, force_state.window_start);

    assert_eq!(anti_poll_status(&marker, "changed", 193, true).0, AntiPollStatus::ForceBlocked);
}

#[test]
fn test_force_escalation_resets_after_window() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let marker = crate::paths::aid_dir().join("board-last.txt");
    write_board_marker(&marker, "fp", 100, 0, 3, 10);

    let (status, force_state) = anti_poll_status(&marker, "changed", 231, true);

    assert_eq!(status, AntiPollStatus::Allowed(0));
    assert_eq!(force_state.count, 1);
    assert_eq!(force_state.window_start, 231);
}

fn make_task(task_id: &str, status: TaskStatus, created_at: chrono::DateTime<Local>) -> Task {
    Task {
        id: TaskId(task_id.to_string()),
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
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at,
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
    }
}
