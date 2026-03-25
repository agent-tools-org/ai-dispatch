// Handler for `aid wait` — block until tasks finish.
// Prints status transitions and per-task completion summaries.
// Deps: crate::store::Store, crate::types, crate::cost

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

use crate::cost;
use crate::store::Store;
use crate::types::{TaskFilter, TaskStatus};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WaitOutcome {
    Completed,
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

    println!("Waiting for {} task(s): {}", task_ids.len(), task_ids.join(", "));
    match wait_for_task_ids(
        store,
        &task_ids,
        tracked_group,
        exit_on_await,
        timeout_secs.map(Duration::from_secs),
    )
    .await?
    {
        WaitOutcome::Completed => Ok(()),
        WaitOutcome::TimedOut(running) => {
            let secs = timeout_secs.unwrap_or_default();
            aid_error!(
                "[aid] Timeout after {}s. Still running: {}",
                secs,
                running.join(", ")
            );
            std::process::exit(124);
        }
    }
}

pub(crate) async fn wait_for_task_ids(
    store: &Arc<Store>,
    task_ids: &[String],
    group: Option<&str>,
    exit_on_await: bool,
    timeout: Option<Duration>,
) -> Result<WaitOutcome> {
    if let Some(timeout) = timeout {
        match tokio::time::timeout(timeout, wait_for_task_ids_inner(store, task_ids, group, exit_on_await)).await {
            Ok(result) => result,
            Err(_) => Ok(WaitOutcome::TimedOut(still_running_task_ids(store, task_ids, group)?)),
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
    let mut completed = 0usize;
    let track_group_tasks = |task_ids: &mut Vec<String>, completed: usize| -> Result<bool> {
        let Some(group_id) = group else { return Ok(false); };
        let mut found = false;
        for task_id in current_running_ids(store, Some(group_id))? {
            if task_ids.contains(&task_id) {
                continue;
            }
            task_ids.push(task_id.clone());
            println!("[{}/{}] new task {} added to watch set", completed, task_ids.len(), task_id);
            found = true;
        }
        Ok(found)
    };

    loop {
        track_group_tasks(&mut task_ids, completed)?;
        let mut remaining = 0usize;
        let total = task_ids.len();

        for task_id in &task_ids {
            let Some(task) = store.get_task(task_id)? else {
                continue;
            };

            let status = task.status;
            let status_text = status.label().to_string();
            let status_changed = last_status.insert(task_id.clone(), status_text.clone()) != Some(status_text);

            // Print status transitions with completion summary
            if status_changed {
                if status.is_terminal() {
                    completed += 1;
                    let duration = task.duration_ms
                        .map(|ms| {
                            let secs = ms / 1000;
                            if secs < 60 { format!("{secs}s") } else { format!("{}m {:02}s", secs / 60, secs % 60) }
                        })
                        .unwrap_or_else(|| "-".to_string());
                    let tokens = task.tokens
                        .map(|t| if t >= 1_000_000 { format!("{:.1}M", t as f64 / 1_000_000.0) } else if t >= 1_000 { format!("{:.1}k", t as f64 / 1_000.0) } else { t.to_string() })
                        .unwrap_or_else(|| "-".to_string());
                    let fail_reason = if status == TaskStatus::Failed {
                        store.latest_error(task_id)
                            .map(|r| format!(" — {r}"))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    println!(
                        "[{}/{}] {} {} ({}, {}tok, {}){fail_reason}",
                        completed, total, task_id, status.label(), duration, tokens, cost::format_cost(task.cost_usd),
                    );
                } else {
                    println!("[{}/{}] {} {}", completed, total, task_id, status.label());
                }
            }

            if exit_on_await && status == TaskStatus::AwaitingInput && status_changed {
                let events = store.get_events(task_id)?;
                let prompt = events
                    .iter()
                    .rev()
                    .find_map(|e| {
                        e.metadata
                            .as_ref()
                            .and_then(|m| m.get("awaiting_prompt"))
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or("");
                println!("{} {}", task_id, prompt);
                println!("Use: aid respond {} \"your answer\"", task_id);
                return Ok(WaitOutcome::Completed);
            }

            if matches!(status, TaskStatus::Pending | TaskStatus::Running | TaskStatus::AwaitingInput) {
                remaining += 1;
            }
        }

        if remaining == 0 {
            if track_group_tasks(&mut task_ids, completed)? {
                continue;
            }
            println!("All {} task(s) completed.", total);
            return Ok(WaitOutcome::Completed);
        }

        sleep(Duration::from_secs(2)).await;
    }
}

fn current_running_ids(store: &Arc<Store>, group: Option<&str>) -> Result<Vec<String>> {
    let mut tasks = store.list_tasks(TaskFilter::Running)?;
    if let Some(group_id) = group {
        tasks.retain(|t| t.workgroup_id.as_deref() == Some(group_id));
    }
    Ok(tasks.into_iter().map(|task| task.id.to_string()).collect())
}

fn still_running_task_ids(store: &Arc<Store>, task_ids: &[String], group: Option<&str>) -> Result<Vec<String>> {
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
        if let Some(task) = store.get_task(&task_id)?
            && !task.status.is_terminal()
        {
            running.push(task_id);
        }
    }
    Ok(running)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use crate::types::{AgentKind, EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus};

    fn make_task(id: &str, status: TaskStatus) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "test prompt".to_string(),
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
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            read_only: false,
            budget: false,
        }
    }

    #[tokio::test]
    async fn wait_for_task_ids_times_out_with_running_tasks() {
        let store = Arc::new(Store::open_memory().unwrap());
        store.insert_task(&make_task("t-run", TaskStatus::Running)).unwrap();
        let outcome = wait_for_task_ids(&store, &[String::from("t-run")], None, false, Some(Duration::from_millis(10)))
            .await
            .unwrap();
        assert_eq!(outcome, WaitOutcome::TimedOut(vec![String::from("t-run")]));
    }

    #[tokio::test]
    async fn wait_for_task_ids_completes_with_existing_milestone_event() {
        let store = Arc::new(Store::open_memory().unwrap());
        let mut task = make_task("t-done", TaskStatus::Done);
        task.duration_ms = Some(1_000);
        store.insert_task(&task).unwrap();
        store.insert_event(&TaskEvent {
            task_id: TaskId("t-done".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: "background progress".to_string(),
            metadata: None,
        }).unwrap();
        let outcome = wait_for_task_ids(&store, &[String::from("t-done")], None, false, None).await.unwrap();
        assert_eq!(outcome, WaitOutcome::Completed);
    }

    #[tokio::test]
    async fn wait_for_task_ids_tracks_group_tasks_added_mid_watch() {
        let store = Arc::new(Store::open_memory().unwrap());
        let mut first = make_task("t-first", TaskStatus::Running);
        first.workgroup_id = Some("wg-dyn".to_string());
        store.insert_task(&first).unwrap();
        let wait_store = store.clone();
        let handle = tokio::spawn(async move { wait_for_task_ids(&wait_store, &[String::from("t-first")], Some("wg-dyn"), false, None).await });
        sleep(Duration::from_millis(100)).await;
        let mut second = make_task("t-second", TaskStatus::Running);
        second.workgroup_id = Some("wg-dyn".to_string());
        store.insert_task(&second).unwrap();
        store.update_task_status("t-first", TaskStatus::Done).unwrap();
        sleep(Duration::from_millis(2_100)).await;
        assert!(!handle.is_finished());
        store.update_task_status("t-second", TaskStatus::Done).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(3), handle).await.unwrap().unwrap().unwrap();
        assert_eq!(outcome, WaitOutcome::Completed);
    }
}
