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
const BOARD_REPEAT_LIMIT: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TruncationNotice {
    shown: usize,
    total: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum AntiPollStatus { Allowed(u32), Cooldown(i64), Repeat(u32) }

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
    let repeat_count = match anti_poll_status(&marker_path, &fingerprint, now, force) {
        AntiPollStatus::Allowed(repeat_count) => {
            if repeat_count > 0 {
                aid_hint!("[aid] No status changes since last check ({repeat_count}x). Use `aid watch --quiet` for automatic notification instead of polling, or `--force` to bypass.");
            }
            repeat_count
        }
        AntiPollStatus::Cooldown(elapsed) => {
            write_board_marker(&marker_path, &fingerprint, now, 0);
            aid_hint!("[aid] Board checked {elapsed}s ago. Use `aid watch --quiet <id>` for live updates, or `--force` to bypass.");
            std::process::exit(1);
        }
        AntiPollStatus::Repeat(repeat_count) => {
            write_board_marker(&marker_path, &fingerprint, now, repeat_count);
            aid_warn!("[aid] No changes after {} checks. Use `aid watch --quiet` instead of polling, or `--force` to bypass. Exiting.", repeat_count);
            std::process::exit(1);
        }
    };
    write_board_marker(&marker_path, &fingerprint, now, repeat_count);

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

fn anti_poll_status(marker_path: &Path, fingerprint: &str, now: i64, force: bool) -> AntiPollStatus {
    if force { return AntiPollStatus::Allowed(0) }
    if let Ok(prev) = std::fs::read_to_string(marker_path) {
        let parts: Vec<&str> = prev.splitn(3, '\n').collect();
        let prev_ts = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let elapsed = now - prev_ts;
        if elapsed >= 0 && elapsed < BOARD_MIN_COOLDOWN_SECS { return AntiPollStatus::Cooldown(elapsed) }
        if parts.get(1).copied().unwrap_or("") == fingerprint {
            let repeat_count = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0) + 1;
            if repeat_count >= BOARD_REPEAT_LIMIT { return AntiPollStatus::Repeat(repeat_count) }
            return AntiPollStatus::Allowed(repeat_count);
        }
    }
    AntiPollStatus::Allowed(0)
}

fn write_board_marker(marker_path: &Path, fingerprint: &str, now: i64, repeat_count: u32) { let _ = std::fs::write(marker_path, format!("{now}\n{fingerprint}\n{repeat_count}")); }

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
    use super::{AntiPollStatus, TruncationNotice, anti_poll_status, apply_limit, board_json_row, long_running_warning, truncation_notice_message};
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
        assert_eq!(anti_poll_status(&marker, "changed", 103, false), AntiPollStatus::Cooldown(3))
    }

    #[test]
    fn test_anti_poll_force_bypasses_cooldown() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        let marker = crate::paths::aid_dir().join("board-last.txt");
        std::fs::write(&marker, "100\nfp\n0").unwrap();
        assert_eq!(anti_poll_status(&marker, "changed", 103, true), AntiPollStatus::Allowed(0))
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
