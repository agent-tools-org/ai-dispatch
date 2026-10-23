// Dependency scheduling phases and final result assembly.
// Deps: parent DependencyRun and batch dispatch helpers.
use super::*;

impl DependencyRun<'_> {
    pub(super) async fn schedule(&mut self) -> Result<()> {
        while self.outcomes.iter().any(Option::is_none) {
            self.dispatch_ready().await?;
            self.expire_waiting().await?;
            if self.active.is_empty() {
                break;
            }
            self.poll_completed().await?;
        }
        Ok(())
    }
    async fn dispatch_ready(&mut self) -> Result<()> {
        let ready = find_ready_tasks(
            &self.store,
            self.tasks,
            self.dependencies,
            &self.started,
            &mut self.outcomes,
            &self.triggered,
        )?;
        self.wait_tracker
            .observe_ready(&ready, chrono::Local::now());
        let max_active = effective_max_active(
            &self.store,
            self.tasks,
            &ready,
            &self.active,
            self.max_concurrent,
        )?;
        let available = max_active.saturating_sub(self.active.len());
        if available > 0 && !ready.is_empty() {
            let dispatch_group: Vec<_> = ready.into_iter().take(available).collect();
            self.dispatch_group(&dispatch_group).await?;
        }
        Ok(())
    }
    async fn dispatch_group(&mut self, dispatch_group: &[usize]) -> Result<()> {
        for dispatch in dispatch_level_with_ids(
            self.store.clone(),
            self.tasks,
            dispatch_group,
            &self.waiting_ids,
            self.shared_dir_path,
            self.repo_root,
        )
        .await?
        {
            self.started[dispatch.index] = true;
            self.wait_tracker.clear(dispatch.index);
            match dispatch.task_id {
                Some(task_id) => {
                    self.task_ids.push(task_id.clone());
                    self.active.push((dispatch.index, task_id));
                }
                None => {
                    self.outcomes[dispatch.index] = Some(BatchTaskOutcome::Failed);
                    // Mark waiting placeholder as skipped
                    let _ = crate::task::lifecycle::mark_skipped(
                        self.store.as_ref(),
                        &self.waiting_ids[dispatch.index],
                    );
                    trigger_conditional(
                        BatchTaskOutcome::Failed,
                        dispatch.index,
                        &mut self.triggered,
                        &self.success_targets,
                        &self.failure_targets,
                    );
                }
            }
        }
        Ok(())
    }
    async fn expire_waiting(&mut self) -> Result<()> {
        for timeout_task_id in self.wait_tracker.fail_expired(
            &self.store,
            &self.waiting_ids,
            &self.started,
            chrono::Local::now(),
        )? {
            let Some(task_idx) = self
                .waiting_ids
                .iter()
                .position(|id| id == &timeout_task_id)
            else {
                continue;
            };
            aid_progress!("[batch] {} TIMEOUT", timeout_task_id);
            self.started[task_idx] = true;
            if let Some(retry_task_id) = maybe_dispatch_auto_fallback(
                self.store.clone(),
                self.tasks,
                task_idx,
                &timeout_task_id,
                BatchTaskOutcome::Failed,
                self.auto_fallback,
                &mut self.retried,
                self.shared_dir_path,
                self.repo_root,
            )
            .await?
            {
                self.task_ids.push(retry_task_id.clone());
                self.active.push((task_idx, retry_task_id));
                continue;
            }
            self.outcomes[task_idx] = Some(BatchTaskOutcome::Failed);
            trigger_conditional(
                BatchTaskOutcome::Failed,
                task_idx,
                &mut self.triggered,
                &self.success_targets,
                &self.failure_targets,
            );
        }
        Ok(())
    }
    async fn poll_completed(&mut self) -> Result<()> {
        let completed_tasks = reconcile_and_poll_completed_tasks(&self.store, &mut self.active)?;
        if completed_tasks.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            return Ok(());
        }
        for completed in completed_tasks {
            self.complete(completed).await?;
        }
        Ok(())
    }
    async fn complete(
        &mut self,
        completed: super::super::super::batch_types::CompletedTask,
    ) -> Result<()> {
        aid_progress!(
            "[batch] {} {}",
            completed.task_id,
            completion_progress_label(self.store.as_ref(), &completed.task_id, completed.outcome)?
        );
        if let Some(retry_task_id) = maybe_dispatch_auto_fallback(
            self.store.clone(),
            self.tasks,
            completed.index,
            &completed.task_id,
            completed.outcome,
            self.auto_fallback,
            &mut self.retried,
            self.shared_dir_path,
            self.repo_root,
        )
        .await?
        {
            self.task_ids.push(retry_task_id.clone());
            self.active.push((completed.index, retry_task_id));
            return Ok(());
        }
        self.outcomes[completed.index] = Some(completed.outcome);
        trigger_conditional(
            completed.outcome,
            completed.index,
            &mut self.triggered,
            &self.success_targets,
            &self.failure_targets,
        );
        Ok(())
    }
    pub(super) fn finish(self) -> Result<BatchDispatchResult> {
        // Mark any remaining waiting tasks as skipped (deps never resolved)
        for (i, outcome) in self.outcomes.iter().enumerate() {
            if outcome.is_none() {
                let _ =
                    crate::task::lifecycle::mark_skipped(self.store.as_ref(), &self.waiting_ids[i]);
            }
        }
        // Only add retry/fallback IDs that aren't already in waiting_ids.
        // Dispatch reuses waiting_ids via existing_task_id, so task_ids often
        // contains duplicates — including them caused 2× cost reporting (#97).
        let waiting_set: std::collections::HashSet<String> =
            self.waiting_ids.iter().cloned().collect();
        let mut all_ids = self.waiting_ids;
        all_ids.extend(
            self.task_ids
                .into_iter()
                .filter(|id| !waiting_set.contains(id)),
        );
        Ok(BatchDispatchResult {
            task_ids: all_ids,
            outcomes: self
                .outcomes
                .into_iter()
                .map(|outcome| outcome.unwrap_or(BatchTaskOutcome::Skipped))
                .collect(),
        })
    }
}
