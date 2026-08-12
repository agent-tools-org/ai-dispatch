// Handler for `aid board` — list all tasks with status summary.
// Detects repeated calls with no status changes and warns callers.
// Deps: crate::store, crate::board, crate::background, project filter.

use anyhow::Result;
use chrono::Local;
use std::io::Write;
use std::sync::Arc;

use crate::background;
use crate::board::render_board;
use crate::store::Store;
use crate::types::{Task, TaskFilter, TaskStatus, Workgroup};

#[path = "board_filter.rs"]
pub(crate) mod board_filter;
#[path = "board_poll.rs"]
mod board_poll;
use board_poll::{
    anti_poll_status, task_fingerprint, watch_instead_of_polling_hint, write_board_marker,
    AntiPollStatus, ForceMarkerState,
};

const DEFAULT_TASK_LIMIT: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TruncationNotice {
    shown: usize,
    total: usize,
}

pub fn run(
    store: &Arc<Store>,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<&str>,
    all_projects: bool,
    limit: Option<usize>,
    force: bool,
    json: bool,
) -> Result<()> {
    let filter = if running {
        TaskFilter::Running
    } else if today {
        TaskFilter::Today
    } else {
        TaskFilter::All
    };

    background::check_zombie_tasks(store)?;
    let mut tasks = store.list_tasks(filter)?;
    let project_filter = board_filter::apply_board_filters(&mut tasks, mine, group, all_projects);
    let truncation = apply_limit(&mut tasks, limit, running, today, mine, group);

    let fingerprint = task_fingerprint(&tasks);
    let marker_path = crate::paths::aid_dir().join("board-last.txt");
    let now = Local::now().timestamp();
    let mut repeat_count = 0;
    let mut force_state = ForceMarkerState::default();
    if !json {
        let anti_poll = anti_poll_status(&marker_path, &fingerprint, now, force);
        force_state = anti_poll.1;
        let watch_hint = watch_instead_of_polling_hint(&tasks);
        repeat_count = match anti_poll.0 {
            AntiPollStatus::Allowed(repeat_count) => repeat_count,
            AntiPollStatus::Cooldown(elapsed) => {
                write_board_marker(&marker_path, &fingerprint, now, 0, 0, 0);
                aid_hint!("[aid] Board checked {elapsed}s ago. {watch_hint}");
                std::process::exit(0);
            }
            AntiPollStatus::Repeat(repeat_count) => {
                write_board_marker(&marker_path, &fingerprint, now, repeat_count, 0, 0);
                aid_warn!("[aid] No changes after {repeat_count} checks. {watch_hint} Exiting.");
                std::process::exit(1);
            }
            AntiPollStatus::ForceCooldown(elapsed) => {
                write_board_marker(&marker_path, &fingerprint, now, 0, force_state.count, force_state.window_start);
                aid_hint!("[aid] Board is rate-limited ({elapsed}s/30s). {watch_hint}");
                std::process::exit(0);
            }
            AntiPollStatus::ForceBlocked => {
                write_board_marker(&marker_path, &fingerprint, now, 0, force_state.count, force_state.window_start);
                aid_warn!("[aid] Repeated polling detected. Board locked for 60s. {watch_hint}");
                std::process::exit(1);
            }
        };
    }
    let mut stdout = std::io::stdout();
    write_board_output(
        &mut stdout,
        store,
        &tasks,
        group,
        project_filter.as_ref().map(|f| f.as_deref()),
        all_projects,
        truncation,
        json,
    )?;
    stdout.flush()?;
    if !json && repeat_count > 0 {
        let watch_hint = watch_instead_of_polling_hint(&tasks);
        aid_hint!("[aid] No status changes since last check ({repeat_count}x). {watch_hint}");
    }
    write_board_marker(&marker_path, &fingerprint, now, repeat_count, force_state.count, force_state.window_start);
    Ok(())
}

fn write_board_output<W: Write>(
    writer: &mut W,
    store: &Store,
    tasks: &[Task],
    group: Option<&str>,
    project_filter: Option<Option<&str>>,
    all_projects: bool,
    truncation: Option<TruncationNotice>,
    json: bool,
) -> Result<()> {
    if json {
        let payload: Vec<serde_json::Value> = tasks.iter().map(board_json_row).collect();
        writeln!(writer, "{}", serde_json::to_string(&payload)?)?;
        return Ok(());
    }
    let has_terminal_worktree = tasks.iter().any(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Failed | TaskStatus::Merged | TaskStatus::Skipped | TaskStatus::Stopped) && task.worktree_path.is_some());
    // Always surface the active project filter so tasks cannot appear to vanish.
    writeln!(
        writer,
        "{}",
        board_filter::project_scope_banner(project_filter, all_projects)
    )?;
    if let Some(group_id) = group
        && let Some(header) = group_header(store, group_id)?
    {
        write!(writer, "{header}")?;
    }
    write!(writer, "{}", render_board(tasks, store)?)?;
    if let Some(notices) = terminal_missing_result_notices(tasks) {
        write!(writer, "{notices}")?;
    }
    if let Some(truncation) = truncation {
        writeln!(writer, "{}", truncation_notice_message(truncation))?;
    }
    if let Some(warning) = long_running_warning(tasks, Local::now()) {
        writeln!(writer, "{warning}")?;
    }
    if has_terminal_worktree
        && let Ok(stale_count) = crate::cmd::worktree::stale_worktree_count(None)
        && stale_count > 3
    {
        writeln!(writer, "[aid] Stale worktrees remain preserved until principal acceptance and custody GC")?;
    }
    Ok(())
}

fn terminal_missing_result_notices(tasks: &[Task]) -> Option<String> {
    let mut notices = String::new();
    for task in tasks {
        if !matches!(task.status, TaskStatus::Done | TaskStatus::Failed) {
            continue;
        }
        let result_path = crate::paths::task_dir(task.id.as_str()).join("result.md");
        if result_path.exists() {
            continue;
        }
        if task.status == TaskStatus::Done {
            notices.push_str(&format!(
                "Status: DONE {} (no result file - see --output / output.md)\n",
                task.id
            ));
        } else {
            notices.push_str(&format!("Status: FAILED {}\n", task.id));
        }
    }
    if notices.is_empty() { None } else { Some(notices) }
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
        "requested_model": task.requested_model, "observed_model": task.observed_model, "attribution_source": task.attribution_source.map(|value| value.as_str()),
        "tokens": task.tokens,
        "duration_ms": task.duration_ms,
        "cost_usd": task.cost_usd,
        "project_id": task.project_id,
        "workgroup_id": task.workgroup_id,
        "worktree_branch": task.worktree_branch,
        "verify_status": task.verify_status.as_str(),
        "delivery_assessment": task.delivery_assessment().map(|delivery| delivery.as_str()),
        "pending_reason": task.pending_reason,
        "created_at": task.created_at.to_rfc3339(),
        "completed_at": task.completed_at.map(|dt| dt.to_rfc3339()),
    })
}

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
