// Handler for `aid board` — list all tasks with status summary.
// Detects repeated calls with no status changes and warns callers.
// Deps: crate::store, crate::board, crate::background, crate::session

use anyhow::Result;
use chrono::Local;
use std::path::Path;
use std::sync::Arc;

use crate::background;
use crate::board::render_board;
use crate::session;
use crate::store::Store;
use crate::types::{Task, TaskFilter, TaskStatus, Workgroup};

const DEFAULT_TASK_LIMIT: usize = 50;
const BOARD_MIN_COOLDOWN_SECS: i64 = 10;
const BOARD_FORCE_COOLDOWN_SECS: i64 = 30;
const BOARD_REPEAT_LIMIT: u32 = 2;
const FORCE_ESCALATION_LIMIT: u32 = 3;
const FORCE_ESCALATION_WINDOW_SECS: i64 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TruncationNotice {
    shown: usize,
    total: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum AntiPollStatus { Allowed(u32), Cooldown(i64), Repeat(u32), ForceCooldown(i64), ForceBlocked }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ForceMarkerState {
    count: u32,
    window_start: i64,
}

pub fn run(store: &Arc<Store>, running: bool, today: bool, mine: bool, group: Option<&str>, limit: Option<usize>, force: bool, json: bool) -> Result<()> {
    let filter = if running {
        TaskFilter::Running
    } else if today {
        TaskFilter::Today
    } else {
        TaskFilter::All
    };

    background::check_zombie_tasks(store)?;
    let mut tasks = store.list_tasks(filter)?;
    if mine {
        tasks.retain(session::matches_current);
    }
    if let Some(group_id) = group {
        tasks.retain(|task| task.workgroup_id.as_deref() == Some(group_id));
    }
    let truncation = apply_limit(&mut tasks, limit, running, today, mine, group);

    let fingerprint = task_fingerprint(&tasks);
    let marker_path = crate::paths::aid_dir().join("board-last.txt");
    let now = Local::now().timestamp();
    let (anti_poll, force_state) = anti_poll_status(&marker_path, &fingerprint, now, force);
    let repeat_count = match anti_poll {
        AntiPollStatus::Allowed(repeat_count) => {
            if repeat_count > 0 {
                aid_hint!("[aid] No status changes since last check ({repeat_count}x). Use `aid watch --quiet` for automatic notification instead of polling.");
            }
            repeat_count
        }
        AntiPollStatus::Cooldown(elapsed) => {
            write_board_marker(&marker_path, &fingerprint, now, 0, 0, 0);
            aid_hint!("[aid] Board checked {elapsed}s ago. Use `aid watch --quiet <id>` for live updates.");
            std::process::exit(1);
        }
        AntiPollStatus::Repeat(repeat_count) => {
            write_board_marker(&marker_path, &fingerprint, now, repeat_count, 0, 0);
            aid_warn!("[aid] No changes after {repeat_count} checks. Use `aid watch --quiet` instead of polling. Exiting.");
            std::process::exit(1);
        }
        AntiPollStatus::ForceCooldown(elapsed) => {
            write_board_marker(&marker_path, &fingerprint, now, 0, force_state.count, force_state.window_start);
            aid_hint!("[aid] Board is rate-limited ({elapsed}s/30s). Use `aid watch --quiet <id>` instead.");
            std::process::exit(1);
        }
        AntiPollStatus::ForceBlocked => {
            write_board_marker(&marker_path, &fingerprint, now, 0, force_state.count, force_state.window_start);
            aid_warn!("[aid] Repeated polling detected. Board locked for 60s. Use `aid watch --quiet <id>` instead.");
            std::process::exit(1);
        }
    };
    write_board_marker(&marker_path, &fingerprint, now, repeat_count, force_state.count, force_state.window_start);

    if json {
        let payload: Vec<serde_json::Value> = tasks.iter().map(board_json_row).collect();
        println!("{}", serde_json::to_string(&payload)?);
        return Ok(());
    }
    let has_terminal_worktree = tasks.iter().any(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Failed | TaskStatus::Merged | TaskStatus::Skipped | TaskStatus::Stopped) && task.worktree_path.is_some());
    if let Some(group_id) = group
        && let Some(header) = group_header(store, group_id)?
    {
        print!("{header}");
    }
    print!("{}", render_board(&tasks, store)?);
    if let Some(truncation) = truncation {
        println!("{}", truncation_notice_message(truncation));
    }
    if let Some(warning) = long_running_warning(&tasks, Local::now()) {
        println!("{warning}");
    }
    if has_terminal_worktree
        && let Ok(stale_count) = crate::cmd::worktree::stale_worktree_count(None)
        && stale_count > 3
    {
        println!("[aid] Tip: run `aid worktree prune` to clean up stale worktrees");
    }
    Ok(())
}

fn group_header(store: &Store, group_id: &str) -> Result<Option<String>> {
    let Some(workgroup) = store.get_workgroup(group_id)? else { return Ok(None) };
    Ok(Some(format_group_header(&workgroup)))
}

fn format_group_header(workgroup: &Workgroup) -> String {
    if workgroup.name == workgroup.id.as_str() { format!("Workgroup: {}\n\n", workgroup.id) } else { format!("Workgroup: {} ({})\n\n", workgroup.id, workgroup.name) }
}

pub(crate) fn apply_limit(tasks: &mut Vec<Task>, limit: Option<usize>, running: bool, today: bool, mine: bool, group: Option<&str>) -> Option<TruncationNotice> {
    let effective_limit = match limit {
        Some(n) => Some(n),
        None if group.is_none() && !running && !today && !mine => Some(DEFAULT_TASK_LIMIT),
        None => None,
    }?;
    if tasks.len() <= effective_limit {
        return None;
    }
    let total = tasks.len();
    tasks.truncate(effective_limit);
    Some(TruncationNotice { shown: effective_limit, total })
}

pub(crate) fn truncation_notice_message(truncation: TruncationNotice) -> String { format!("[aid] Showing {} of {} tasks. Use --limit N or --today/--running for more.", truncation.shown, truncation.total) }

/// Compact fingerprint of task statuses for change detection.
fn task_fingerprint(tasks: &[crate::types::Task]) -> String {
    let mut parts: Vec<String> = tasks.iter().map(|t| format!("{}:{}", t.id, t.status.label())).collect();
    parts.sort();
    parts.join(",")
}

fn anti_poll_status(marker_path: &Path, fingerprint: &str, now: i64, force: bool) -> (AntiPollStatus, ForceMarkerState) {
    let marker = read_board_marker(marker_path);
    let elapsed = now - marker.timestamp;
    if force {
        let force_state = next_force_state(&marker, now);
        if elapsed >= 0 && elapsed < BOARD_FORCE_COOLDOWN_SECS { return (AntiPollStatus::ForceCooldown(elapsed), force_state) }
        if is_force_window_active(marker.force_window_start, now) && marker.force_count >= FORCE_ESCALATION_LIMIT {
            return (AntiPollStatus::ForceBlocked, force_state);
        }
        return (AntiPollStatus::Allowed(0), force_state);
    }
    if elapsed >= 0 && elapsed < BOARD_MIN_COOLDOWN_SECS { return (AntiPollStatus::Cooldown(elapsed), ForceMarkerState::default()) }
    if marker.fingerprint == fingerprint {
        let repeat_count = marker.repeat_count + 1;
        if repeat_count >= BOARD_REPEAT_LIMIT { return (AntiPollStatus::Repeat(repeat_count), ForceMarkerState::default()) }
        return (AntiPollStatus::Allowed(repeat_count), ForceMarkerState::default());
    }
    (AntiPollStatus::Allowed(0), ForceMarkerState::default())
}

#[derive(Debug, Default)]
struct BoardMarker {
    timestamp: i64,
    fingerprint: String,
    repeat_count: u32,
    force_count: u32,
    force_window_start: i64,
}

fn read_board_marker(marker_path: &Path) -> BoardMarker {
    let Ok(prev) = std::fs::read_to_string(marker_path) else { return BoardMarker::default() };
    let parts: Vec<&str> = prev.lines().collect();
    BoardMarker {
        timestamp: parts.first().and_then(|s| s.parse().ok()).unwrap_or(0),
        fingerprint: parts.get(1).copied().unwrap_or("").to_string(),
        repeat_count: parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
        force_count: parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
        force_window_start: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
    }
}

fn next_force_state(marker: &BoardMarker, now: i64) -> ForceMarkerState {
    if is_force_window_active(marker.force_window_start, now) {
        if marker.force_count >= FORCE_ESCALATION_LIMIT {
            return ForceMarkerState { count: marker.force_count, window_start: marker.force_window_start };
        }
        return ForceMarkerState { count: marker.force_count + 1, window_start: marker.force_window_start };
    }
    ForceMarkerState { count: 1, window_start: now }
}

fn is_force_window_active(force_window_start: i64, now: i64) -> bool {
    force_window_start > 0 && now - force_window_start >= 0 && now - force_window_start < FORCE_ESCALATION_WINDOW_SECS
}

fn write_board_marker(marker_path: &Path, fingerprint: &str, now: i64, repeat_count: u32, force_count: u32, force_window_start: i64) {
    let _ = std::fs::write(marker_path, format!("{now}\n{fingerprint}\n{repeat_count}\n{force_count}\n{force_window_start}"));
}

fn long_running_warning(tasks: &[crate::types::Task], now: chrono::DateTime<Local>) -> Option<String> {
    let count = tasks.iter().filter(|task| task.status == TaskStatus::Running).filter(|task| (now - task.created_at).num_hours() >= 1).count();
    if count == 0 { return None }
    Some(format!("[aid] Warning: {} task(s) running >1h — may be stale. Use `aid stop <id>` to clean up.", count))
}

fn board_json_row(task: &Task) -> serde_json::Value {
    serde_json::json!({
        "id": task.id.as_str(),
        "agent": task.agent_display_name(),
        "status": task.status.as_str(),
        "prompt": task.prompt,
        "model": task.model,
        "tokens": task.tokens,
        "duration_ms": task.duration_ms,
        "cost_usd": task.cost_usd,
        "workgroup_id": task.workgroup_id,
        "worktree_branch": task.worktree_branch,
        "verify_status": task.verify_status.as_str(),
        "pending_reason": task.pending_reason,
        "created_at": task.created_at.to_rfc3339(),
        "completed_at": task.completed_at.map(|dt| dt.to_rfc3339()),
    })
}

#[cfg(test)]
mod tests {
    use super::{AntiPollStatus, TruncationNotice, anti_poll_status, apply_limit, board_json_row, long_running_warning, truncation_notice_message, write_board_marker};
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
}
