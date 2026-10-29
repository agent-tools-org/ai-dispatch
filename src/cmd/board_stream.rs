// Streaming board output for `aid board --stream`.
// Exports run() to print task status updates to stdout.
// Depends on Store lookups and tokio timing utilities.

use crate::ui::format::elapsed_since;
use anyhow::Result;
use chrono::Local;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::runtime::background;
use crate::cmd::eta;
use crate::store::Store;
use crate::types::{Task, TaskOutcome, TaskStatus};
use crate::ui::format::format_duration_ms;
use crate::ui::format::truncate;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const PROMPT_MAX: usize = 60;
const ANSI_RESET: &str = "\x1b[0m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_BLUE: &str = "\x1b[34m";
const ANSI_DIM: &str = "\x1b[2m";
pub async fn run(
    store: &Arc<Store>,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<&str>,
    all_projects: bool,
    limit: Option<usize>,
) -> Result<()> {
    background::check_zombie_tasks(store)?;
    let mut init = init_stream(store, running, today, mine, group, all_projects, limit)?;
    if init.tasks.is_empty() || init.tasks.iter().all(|task| is_terminal(task.status)) {
        print_summary(&init.tasks, "Summary");
        return Ok(());
    }
    run_stream_loop(
        store,
        running,
        today,
        mine,
        group,
        all_projects,
        limit,
        &mut init.state,
    )
    .await
}

struct StreamState {
    last_status: HashMap<String, TaskStatus>,
}

struct StreamInit {
    state: StreamState,
    tasks: Vec<Task>,
}

struct FilteredTasks {
    tasks: Vec<Task>,
    truncation: Option<crate::cmd::board::TruncationNotice>,
}

enum StreamAction {
    Continue,
    Exit,
}

fn init_stream(
    store: &Store,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<&str>,
    all_projects: bool,
    limit: Option<usize>,
) -> Result<StreamInit> {
    let current = crate::config::project::current_project_id();
    println!(
        "{}",
        crate::cmd::board::board_filter::project_scope_banner(
            if all_projects { None } else { Some(current.as_deref()) },
            all_projects,
        )
    );
    println!("ID | Route | Status | Duration | Prompt (truncated)");
    let FilteredTasks { mut tasks, truncation } =
        list_filtered_tasks(store, running, today, mine, group, all_projects, limit)?;
    tasks.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    let mut last_status = HashMap::new();
    for task in tasks.iter().filter(|task| is_active(task.status)) {
        last_status.insert(task.id.as_str().to_string(), task.status);
        println!(
            "{} | {} | {} | {} | {}",
            task.id.as_str(),
            task.display_route(),
            colored_status(task.status),
            duration_for_task(task, store),
            prompt_snippet(task),
        );
    }
    if let Some(truncation) = truncation {
        println!("{}", crate::cmd::board::truncation_notice_message(truncation));
    }
    Ok(StreamInit {
        state: StreamState { last_status },
        tasks,
    })
}

async fn run_stream_loop(
    store: &Store,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<&str>,
    all_projects: bool,
    limit: Option<usize>,
    state: &mut StreamState,
) -> Result<()> {
    let mut ticker = tokio::time::interval(POLL_INTERVAL);
    ticker.tick().await;
    loop {
        ticker.tick().await;
        match poll_and_print(store, running, today, mine, group, all_projects, limit, state)? {
            StreamAction::Continue => {}
            StreamAction::Exit => return Ok(()),
        }
    }
}

fn poll_and_print(
    store: &Store,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<&str>,
    all_projects: bool,
    limit: Option<usize>,
    state: &mut StreamState,
) -> Result<StreamAction> {
    let mut tasks =
        list_filtered_tasks(store, running, today, mine, group, all_projects, limit)?.tasks;
    tasks.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    for task in &tasks {
        let key = task.id.as_str();
        let status_changed = state
            .last_status
            .get(key)
            .map(|status| *status != task.status)
            .unwrap_or(true);
        if status_changed {
            state.last_status.insert(key.to_string(), task.status);
            println!(
                "[{}] {} {} {} {} {}",
                Local::now().format("%H:%M:%S"),
                task.id.as_str(),
                task.display_route(),
                colored_status(task.status),
                duration_for_task(task, store),
                prompt_snippet(task),
            );
        }
    }

    if tasks.is_empty() {
        print_summary(&tasks, "Summary");
        return Ok(StreamAction::Exit);
    }

    if tasks.iter().all(|task| is_terminal(task.status)) {
        print_summary(&tasks, "Summary");
        return Ok(StreamAction::Exit);
    }

    Ok(StreamAction::Continue)
}

fn list_filtered_tasks(
    store: &Store, running: bool, today: bool, mine: bool, group: Option<&str>,
    all_projects: bool, limit: Option<usize>,
) -> Result<FilteredTasks> {
    let (tasks, _, truncation) = crate::cmd::board::board_filter::load_board_tasks(
        store, running, today, mine, group, all_projects, limit,
    )?;
    Ok(FilteredTasks { tasks, truncation })
}

fn is_active(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Pending | TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled
    )
}

fn is_terminal(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Done | TaskStatus::Failed | TaskStatus::Merged | TaskStatus::Skipped | TaskStatus::Stopped
    )
}

fn colored_status(status: TaskStatus) -> String {
    match status {
        TaskStatus::Done => color("Done", ANSI_GREEN),
        TaskStatus::Merged => color("Merged", ANSI_GREEN),
        TaskStatus::Failed => color("Failed", ANSI_RED),
        TaskStatus::Stopped => color("Stopped", ANSI_YELLOW),
        TaskStatus::Running => color("Running", ANSI_YELLOW),
        TaskStatus::AwaitingInput => color("Await", ANSI_BLUE),
        TaskStatus::Stalled => color("Stalled", ANSI_RED),
        TaskStatus::Waiting => color("Waiting", ANSI_DIM),
        TaskStatus::Pending => color("Queued", ANSI_DIM),
        TaskStatus::Skipped => color("Skipped", ANSI_DIM),
    }
}

fn color(label: &str, code: &str) -> String {
    format!("{code}{label}{ANSI_RESET}")
}

fn duration_for_task(task: &Task, store: &Store) -> String {
    if task.status == TaskStatus::Skipped {
        return "-".to_string();
    }
    let duration = task
        .duration_ms
        .map(format_duration_ms)
        .unwrap_or_else(|| elapsed_since(task.created_at));
    match eta::estimate_eta(task, store) {
        Some(eta) => format!("{duration} (ETA: {eta})"),
        None => duration,
    }
}

fn prompt_snippet(task: &Task) -> String {
    let normalized = task
        .prompt
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    truncate(&normalized, PROMPT_MAX)
}

fn print_summary(tasks: &[Task], label: &str) {
    let total = tasks.len();
    let done = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Done && t.outcome().is_success())
        .count();
    let merged = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Merged && t.outcome().is_success())
        .count();
    let failed = tasks
        .iter()
        .filter(|t| matches!(t.outcome(), TaskOutcome::Broken | TaskOutcome::Failed))
        .count();
    let skipped = tasks
        .iter()
        .filter(|t| matches!(t.outcome(), TaskOutcome::Skipped))
        .count();
    println!(
        "{label}: {total} total | {done} done | {merged} merged | {failed} failed | {skipped} skipped"
    );
}
