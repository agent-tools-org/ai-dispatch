// Dependency-aware batch scheduling and fallback orchestration.
// Deps: parent batch dispatch helpers and state types.
use super::*;

pub(super) async fn dispatch_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    dependencies: &[Vec<usize>],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
    repo_root: Option<&str>,
) -> Result<BatchDispatchResult> {
    if tasks.is_empty() {
        return Ok(BatchDispatchResult {
            task_ids: Vec::new(),
            outcomes: Vec::new(),
        });
    }
    warn_rate_limited_agents(tasks);
    // Pre-create all tasks with Waiting status so they're visible in TUI immediately
    let waiting_ids: Vec<String> = tasks
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let id = task
                .id
                .as_ref()
                .filter(|s| crate::support::sanitize::is_valid_task_id(s))
                .map(|s| crate::types::TaskId(s.clone()))
                .unwrap_or_else(crate::types::TaskId::generate);
            let agent = task.agent.as_str();
            if let Err(e) = store.insert_waiting_task(
                id.as_str(),
                agent,
                &task.prompt,
                None,
                task.group.as_deref(),
                task.dir.as_deref(),
                task.worktree.as_deref(),
                task.model.as_deref(),
                task.verify.as_deref(),
                task.read_only,
                task.budget
                    .is_some_and(crate::types::TaskBudget::uses_budget_mode),
            ) {
                aid_warn!("[aid] Warning: failed to pre-create task {i}: {e}");
            }
            id.to_string()
        })
        .collect();
    let name_map = batch::task_name_map(tasks)?;
    let success_targets =
        resolve_hook_targets(tasks, &name_map, |task| task.on_success.as_deref())?;
    let failure_targets = resolve_hook_targets(tasks, &name_map, |task| task.on_fail.as_deref())?;
    let mut started = vec![false; tasks.len()];
    let mut outcomes: Vec<Option<BatchTaskOutcome>> = vec![None; tasks.len()];
    let mut retried = vec![false; tasks.len()];
    let mut triggered: Vec<bool> = tasks.iter().map(|task| !task.conditional).collect();
    let mut active: Vec<(usize, String)> = Vec::new();
    let mut task_ids: Vec<String> = Vec::new();
    let mut wait_tracker = ReadyWaitTracker::new(tasks);
    while outcomes.iter().any(Option::is_none) {
        let ready = find_ready_tasks(
            &store,
            tasks,
            dependencies,
            &started,
            &mut outcomes,
            &triggered,
        )?;
        wait_tracker.observe_ready(&ready, chrono::Local::now());
        let max_active = effective_max_active(&store, tasks, &ready, &active, max_concurrent)?;
        let available = max_active.saturating_sub(active.len());
        if available > 0 && !ready.is_empty() {
            let dispatch_group: Vec<_> = ready.into_iter().take(available).collect();
            for dispatch in dispatch_level_with_ids(
                store.clone(),
                tasks,
                &dispatch_group,
                &waiting_ids,
                shared_dir_path,
                repo_root,
            )
            .await?
            {
                started[dispatch.index] = true;
                wait_tracker.clear(dispatch.index);
                match dispatch.task_id {
                    Some(task_id) => {
                        task_ids.push(task_id.clone());
                        active.push((dispatch.index, task_id));
                    }
                    None => {
                        outcomes[dispatch.index] = Some(BatchTaskOutcome::Failed);
                        // Mark waiting placeholder as skipped
                        let _ = crate::task::lifecycle::mark_skipped(
                            store.as_ref(),
                            &waiting_ids[dispatch.index],
                        );
                        trigger_conditional(
                            BatchTaskOutcome::Failed,
                            dispatch.index,
                            &mut triggered,
                            &success_targets,
                            &failure_targets,
                        );
                    }
                }
            }
        }
        for timeout_task_id in
            wait_tracker.fail_expired(&store, &waiting_ids, &started, chrono::Local::now())?
        {
            let Some(task_idx) = waiting_ids.iter().position(|id| id == &timeout_task_id) else {
                continue;
            };
            aid_progress!("[batch] {} TIMEOUT", timeout_task_id);
            started[task_idx] = true;
            if let Some(retry_task_id) = maybe_dispatch_auto_fallback(
                store.clone(),
                tasks,
                task_idx,
                &timeout_task_id,
                BatchTaskOutcome::Failed,
                auto_fallback,
                &mut retried,
                shared_dir_path,
                repo_root,
            )
            .await?
            {
                task_ids.push(retry_task_id.clone());
                active.push((task_idx, retry_task_id));
                continue;
            }
            outcomes[task_idx] = Some(BatchTaskOutcome::Failed);
            trigger_conditional(
                BatchTaskOutcome::Failed,
                task_idx,
                &mut triggered,
                &success_targets,
                &failure_targets,
            );
        }
        if active.is_empty() {
            break;
        }
        let completed_tasks = reconcile_and_poll_completed_tasks(&store, &mut active)?;
        if completed_tasks.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            continue;
        }
        for completed in completed_tasks {
            aid_progress!(
                "[batch] {} {}",
                completed.task_id,
                completion_progress_label(store.as_ref(), &completed.task_id, completed.outcome)?
            );
            if let Some(retry_task_id) = maybe_dispatch_auto_fallback(
                store.clone(),
                tasks,
                completed.index,
                &completed.task_id,
                completed.outcome,
                auto_fallback,
                &mut retried,
                shared_dir_path,
                repo_root,
            )
            .await?
            {
                task_ids.push(retry_task_id.clone());
                active.push((completed.index, retry_task_id));
                continue;
            }
            outcomes[completed.index] = Some(completed.outcome);
            trigger_conditional(
                completed.outcome,
                completed.index,
                &mut triggered,
                &success_targets,
                &failure_targets,
            );
        }
    }
    // Mark any remaining waiting tasks as skipped (deps never resolved)
    for (i, outcome) in outcomes.iter().enumerate() {
        if outcome.is_none() {
            let _ = crate::task::lifecycle::mark_skipped(store.as_ref(), &waiting_ids[i]);
        }
    }
    // Only add retry/fallback IDs that aren't already in waiting_ids.
    // Dispatch reuses waiting_ids via existing_task_id, so task_ids often
    // contains duplicates — including them caused 2× cost reporting (#97).
    let waiting_set: std::collections::HashSet<String> = waiting_ids.iter().cloned().collect();
    let mut all_ids = waiting_ids;
    all_ids.extend(task_ids.into_iter().filter(|id| !waiting_set.contains(id)));
    Ok(BatchDispatchResult {
        task_ids: all_ids,
        outcomes: outcomes
            .into_iter()
            .map(|outcome| outcome.unwrap_or(BatchTaskOutcome::Skipped))
            .collect(),
    })
}
