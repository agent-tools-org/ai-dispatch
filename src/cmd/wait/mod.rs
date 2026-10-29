// Handler for `aid wait` — block until tasks finish.
// Prints status transitions and per-task completion summaries.
// Deps: crate::store::Store, crate::types, crate::cost

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{Duration, Instant, sleep};

use crate::cost;
use crate::store::Store;
use crate::task::verify::VERIFY_TIMEOUT;
use crate::types::{Task, TaskFilter, TaskOutcome, TaskStatus, VerifyStatus, verify_required};
#[path = "../wait_settlement.rs"]
mod settlement;
use settlement::worker_is_settling;
#[path = "../wait_json.rs"]
mod json;
pub(crate) use json::run_json;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WaitOutcome {
    Completed,
    Failed(Vec<String>),
    TimedOut(Vec<String>),
}

pub async fn run(
    store: &Arc<Store>,
    task_ids: &[String],
    group: Option<&str>,
    exit_on_await: bool,
    timeout_secs: Option<u64>,
) -> Result<()> {
    let tracked_group = if task_ids.is_empty() { group } else { None };
    let task_ids = if task_ids.is_empty() {
        current_running_ids(store, tracked_group)?
    } else {
        task_ids.to_vec()
    };

    if task_ids.is_empty() {
        println!("No running tasks.");
        return Ok(());
    }

    println!(
        "Waiting for {} task(s): {}",
        task_ids.len(),
        task_ids.join(", ")
    );
    let outcome = wait_for_task_ids(
        store,
        &task_ids,
        tracked_group,
        exit_on_await,
        timeout_secs.map(Duration::from_secs),
    )
    .await?;
    report_wait_outcome(outcome, tracked_group, timeout_secs)
}

pub(crate) async fn wait_for_task_ids(
    store: &Arc<Store>,
    task_ids: &[String],
    group: Option<&str>,
    exit_on_await: bool,
    timeout: Option<Duration>,
) -> Result<WaitOutcome> {
    if let Some(timeout) = timeout {
        match tokio::time::timeout(
            timeout,
            wait_for_task_ids_inner(store, task_ids, group, exit_on_await),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Ok(WaitOutcome::TimedOut(still_running_task_ids(
                store, task_ids, group,
            )?)),
        }
    } else {
        wait_for_task_ids_inner(store, task_ids, group, exit_on_await).await
    }
}

async fn wait_for_task_ids_inner(
    store: &Arc<Store>,
    task_ids: &[String],
    group: Option<&str>,
    exit_on_await: bool,
) -> Result<WaitOutcome> {
    let mut task_ids = task_ids.to_vec();
    let mut last_status: HashMap<String, String> = HashMap::new();
    let mut pending_since: HashMap<String, Instant> = HashMap::new();
    let mut completed = 0usize;

    loop {
        let _ = crate::runtime::background::check_zombie_tasks(store);
        track_group_tasks(store, group, &mut task_ids, completed)?;
        let total = task_ids.len();
        let (remaining, mut failed, awaiting) = poll_wait_tasks(
            store,
            &task_ids,
            exit_on_await,
            &mut last_status,
            &mut pending_since,
            &mut completed,
        )?;
        if awaiting {
            return Ok(WaitOutcome::Completed);
        }

        if remaining == 0 {
            if track_group_tasks(store, group, &mut task_ids, completed)? {
                continue;
            }
            println!("All {} task(s) completed.", total);
            failed.sort();
            failed.dedup();
            return Ok(if failed.is_empty() {
                WaitOutcome::Completed
            } else {
                WaitOutcome::Failed(failed)
            });
        }

        sleep(Duration::from_secs(2)).await;
    }
}

fn current_running_ids(store: &Arc<Store>, group: Option<&str>) -> Result<Vec<String>> {
    let mut tasks = store.list_tasks(TaskFilter::Active)?;
    settlement::include_worker_tasks(store, &mut tasks)?;
    if let Some(group_id) = group {
        tasks.retain(|t| t.workgroup_id.as_deref() == Some(group_id));
    }
    Ok(tasks.into_iter().map(|task| task.id.to_string()).collect())
}

fn still_running_task_ids(
    store: &Arc<Store>,
    task_ids: &[String],
    group: Option<&str>,
) -> Result<Vec<String>> {
    let mut tracked = task_ids.to_vec();
    if let Some(group_id) = group {
        for task_id in current_running_ids(store, Some(group_id))? {
            if !tracked.contains(&task_id) {
                tracked.push(task_id);
            }
        }
    }
    let mut running = Vec::new();
    for task_id in tracked {
        if worker_is_settling(&task_id)? {
            running.push(task_id);
            continue;
        }
        if let Some(task) = store.get_task(&task_id)? {
            if verification_is_in_flight(&task) {
                running.push(task_id);
                continue;
            }
            let outcome = task.outcome();
            if matches!(outcome, TaskOutcome::InProgress) {
                running.push(task_id);
            }
        }
    }
    Ok(running)
}
fn verification_is_in_flight(task: &Task) -> bool {
    matches!(task.status, TaskStatus::Done | TaskStatus::Merged)
        && task.verify_status == VerifyStatus::Pending
        && verify_required(task.verify.as_deref())
}

#[cfg(test)]
#[path = "../wait_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../wait_settlement_tests.rs"]
mod settlement_tests;

fn track_group_tasks(
    store: &Arc<Store>,
    group: Option<&str>,
    task_ids: &mut Vec<String>,
    completed: usize,
) -> Result<bool> {
    let Some(group_id) = group else {
        return Ok(false);
    };
    let mut found = false;
    for task_id in current_running_ids(store, Some(group_id))? {
        if task_ids.contains(&task_id) {
            continue;
        }
        task_ids.push(task_id.clone());
        println!(
            "[{}/{}] new task {} added to watch set",
            completed,
            task_ids.len(),
            task_id
        );
        found = true;
    }
    Ok(found)
}

fn poll_wait_tasks(
    store: &Store,
    task_ids: &[String],
    exit_on_await: bool,
    last_status: &mut HashMap<String, String>,
    pending_since: &mut HashMap<String, Instant>,
    completed: &mut usize,
) -> Result<(usize, Vec<String>, bool)> {
    let mut remaining = 0usize;
    let total = task_ids.len();
    let mut failed = Vec::new();

    for task_id in task_ids {
        // The worker removes its spec only after delivery/verification settle.
        // Observe that barrier BEFORE reading status, never reuse a pre-barrier Done.
        let settling = worker_is_settling(task_id)?;
        let Some(task) = store.get_task(task_id)? else {
            continue;
        };
        if settling && task.status.is_terminal() {
            remaining += 1;
            continue;
        }
        let status = task.status;
        let status_text = status.label().to_string();
        let status_changed =
            last_status.insert(task_id.clone(), status_text.clone()) != Some(status_text);

        // Print status transitions with completion summary
        if status_changed {
            print_task_transition(store, task_id, &task, completed, total);
        }

        if exit_on_await && status == TaskStatus::AwaitingInput && status_changed {
            print_waiting_prompt(store, task_id)?;
            return Ok((remaining, failed, true));
        }

        if task_remains_pending(&task, task_id, pending_since, &mut failed) {
            remaining += 1;
        }
    }

    Ok((remaining, failed, false))
}

fn task_remains_pending(
    task: &Task,
    task_id: &str,
    pending_since: &mut HashMap<String, Instant>,
    failed: &mut Vec<String>,
) -> bool {
    let outcome = task.outcome();
    if verification_is_in_flight(task) {
        let started_at = pending_since
            .entry(task_id.to_string())
            .or_insert_with(Instant::now);
        if started_at.elapsed() < VERIFY_TIMEOUT {
            return true;
        } else {
            failed.push(task_id.to_string());
        }
    } else if matches!(outcome, TaskOutcome::InProgress) {
        pending_since.remove(task_id);
        return true;
    } else if !outcome.is_success() {
        pending_since.remove(task_id);
        failed.push(task_id.to_string());
    } else {
        pending_since.remove(task_id);
    }
    false
}

#[path = "report.rs"]
mod report;
use report::*;
