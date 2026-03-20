// Streaming board output for `aid board --stream`.
// Exports run() to print task status updates to stdout.
// Depends on Store lookups and tokio timing utilities.

use anyhow::Result;
use chrono::Local;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::background;
use crate::cmd::eta;
use crate::session;
use crate::store::Store;
use crate::types::{Task, TaskFilter, TaskStatus};

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
) -> Result<()> {
    background::check_zombie_tasks(store)?;
    let mut init = init_stream(store, running, today, mine, group)?;
    if init.tasks.is_empty() || init.tasks.iter().all(|task| is_terminal(task.status)) {
        print_summary(&init.tasks, "Summary");
        return Ok(());
    }
    run_stream_loop(store, running, today, mine, group, &mut init.state).await
}

struct StreamState {
    last_status: HashMap<String, TaskStatus>,
}

struct StreamInit {
    state: StreamState,
    tasks: Vec<Task>,
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
) -> Result<StreamInit> {
    println!("ID | Agent | Status | Duration | Prompt (truncated)");
    let mut tasks = list_filtered_tasks(store, running, today, mine, group)?;
    tasks.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    let mut last_status = HashMap::new();
    for task in tasks.iter().filter(|task| is_active(task.status)) {
        last_status.insert(task.id.as_str().to_string(), task.status);
        println!(
            "{} | {} | {} | {} | {}",
            task.id.as_str(),
            task.agent_display_name(),
            colored_status(task.status),
            duration_for_task(task, store),
            prompt_snippet(task),
        );
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
    state: &mut StreamState,
) -> Result<()> {
    let mut ticker = tokio::time::interval(POLL_INTERVAL);
    ticker.tick().await;
    loop {
        ticker.tick().await;
        match poll_and_print(store, running, today, mine, group, state)? {
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
    state: &mut StreamState,
) -> Result<StreamAction> {
    let mut tasks = list_filtered_tasks(store, running, today, mine, group)?;
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
                task.agent_display_name(),
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
    store: &Store,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<&str>,
) -> Result<Vec<Task>> {
    let filter = if running {
        TaskFilter::Running
    } else if today {
        TaskFilter::Today
    } else {
        TaskFilter::All
    };

    let mut tasks = store.list_tasks(filter)?;
    if mine {
        tasks.retain(session::matches_current);
    }
    if let Some(group_id) = group {
        tasks.retain(|task| task.workgroup_id.as_deref() == Some(group_id));
    }
    Ok(tasks)
}

fn is_active(status: TaskStatus) -> bool {
    matches!(status, TaskStatus::Pending | TaskStatus::Running | TaskStatus::AwaitingInput)
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
        TaskStatus::Stopped => color("Stopped", ANSI_RED),
        TaskStatus::Running => color("Running", ANSI_YELLOW),
        TaskStatus::AwaitingInput => color("Await", ANSI_BLUE),
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
        .map(format_duration)
        .unwrap_or_else(|| elapsed_since(task.created_at));
    match eta::estimate_eta(task, store) {
        Some(eta) => format!("{duration} (ETA: {eta})"),
        None => duration,
    }
}

fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

fn elapsed_since(start: chrono::DateTime<chrono::Local>) -> String {
    let elapsed = chrono::Local::now() - start;
    let secs = elapsed.num_seconds();
    if secs < 0 {
        "0s".to_string()
    } else if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let safe = s.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &s[..safe])
    }
}

fn print_summary(tasks: &[Task], label: &str) {
    let total = tasks.len();
    let done = tasks.iter().filter(|t| t.status == TaskStatus::Done).count();
    let merged = tasks.iter().filter(|t| t.status == TaskStatus::Merged).count();
    let failed = tasks.iter().filter(|t| t.status == TaskStatus::Failed).count();
    let skipped = tasks.iter().filter(|t| t.status == TaskStatus::Skipped).count();
    println!(
        "{label}: {total} total | {done} done | {merged} merged | {failed} failed | {skipped} skipped"
    );
}
