// Completion effects and continuation phases for the lifecycle run state.
// Depends on the parent lifecycle coordinator and existing lifecycle helpers.
use super::*;

impl LifecycleRun<'_> {
    async fn peer_review(&mut self) -> Result<()> {
        if self.store.is_caller_owned(self.task_id.as_str())? {
            return Ok(());
        }
        if let Some(ref reviewer_agent) = self.args.peer_review
            && let Some(task) = self.store.get_task(self.task_id.as_str())?
            && task.status == TaskStatus::Done
        {
            match judge::peer_review_task(&task, reviewer_agent, &self.args.prompt).await {
                Ok(review) => {
                    aid_info!(
                        "[aid] Peer review by {reviewer_agent}: {}/10 — {}",
                        review.score,
                        review.feedback
                    );
                    self.store.save_peer_review(
                        self.task_id.as_str(),
                        reviewer_agent,
                        review.score,
                        &review.feedback,
                    )?;
                }
                Err(e) => aid_error!("[aid] Peer review failed: {e}"),
            }
        }
        Ok(())
    }
    fn persist_output(&mut self) -> Result<()> {
        run_prompt::notify_task_completion(self.store, self.task_id)?;
        if let Some(task) = self.store.get_task(self.task_id.as_str())?
            && task.output_path.is_none()
        {
            auto_save_task_output(self.store.as_ref(), &task)?;
        }
        if let Some(task) = self.store.get_task(self.task_id.as_str())? {
            maybe_flag_hollow_output(
                self.store.as_ref(),
                self.task_id,
                &task,
                self.args.base_branch.as_deref(),
            );
        }
        maybe_run_post_done_audit(
            self.store.as_ref(),
            self.task_id,
            self.args,
            self.effective_dir.map(String::as_str),
            self.repo_path.map(String::as_str),
        )?;
        Ok(())
    }
    pub(super) async fn complete(&mut self) -> Result<ControlFlow<Option<TaskId>>> {
        self.peer_review().await?;
        self.persist_output()?;
        let Some(task) = self.store.get_task(self.task_id.as_str())? else {
            return Ok(ControlFlow::Break(None));
        };
        let summary_json = serde_json::to_string(&crate::cmd::summary::generate_summary(&task))
            .unwrap_or_default();
        let _ = self
            .store
            .save_completion_summary(self.task_id.as_str(), &summary_json);
        if let Some(task) = self.store.get_task(self.task_id.as_str())? {
            let done_payload = show::task_hook_json(
                &task,
                self.agent_display_name,
                self.effective_dir.map(String::as_str),
            );
            if let Err(err) = hooks::run_hooks_with(
                "after_complete",
                &done_payload,
                Some(self.agent_display_name),
                self.runtime_hooks,
                false,
            ) {
                aid_error!("[aid] Hook after_complete failed: {err}");
            }
        }
        crate::task::webhook::fire_task_webhooks(self.store, self.task_id.as_str()).await;
        self.announce()?;
        Ok(ControlFlow::Continue(()))
    }
    fn announce(&mut self) -> Result<()> {
        if self.mode.is_foreground() && self.args.announce && !self.args.background {
            if let Some(status_hint) =
                foreground_status_hint(self.store.as_ref(), self.task_id.as_str())?
            {
                aid_hint!("{status_hint}");
            }
        }
        Ok(())
    }
    pub(super) async fn retry_verification(
        &mut self,
        pre_verify_status: TaskStatus,
    ) -> Result<ControlFlow<Option<TaskId>>> {
        let iterate_config = iterate_config(self.args)?;
        if let Some(iterate_config) = iterate_config.as_ref()
            && let Some(retry_id) =
                maybe_iterate(self.store, self.task_id, self.args, iterate_config).await?
        {
            return Ok(ControlFlow::Break(Some(retry_id)));
        }
        if iterate_config.is_none() {
            if let Some(retry_id) = maybe_auto_retry_after_verify_failure(
                self.store,
                self.task_id,
                self.args,
                pre_verify_status,
            )
            .await?
            {
                return Ok(ControlFlow::Break(Some(retry_id)));
            }
            if let Some(retry_id) = maybe_auto_retry_after_checklist_miss(
                self.store,
                self.task_id,
                self.args,
                self.checklist_result.as_ref(),
            )
            .await?
            {
                return Ok(ControlFlow::Break(Some(retry_id)));
            }
        }
        Ok(ControlFlow::Continue(()))
    }
    pub(super) async fn retry_execution(&mut self) -> Result<ControlFlow<Option<TaskId>>> {
        if let Some(retry_id) =
            maybe_auto_retry_after_hang(self.store, self.task_id, self.args).await?
        {
            return Ok(ControlFlow::Break(Some(retry_id)));
        }
        if let Some(retry_id) =
            maybe_auto_retry_after_model_unavailable(self.store, self.task_id, self.args).await?
        {
            return Ok(ControlFlow::Break(Some(retry_id)));
        }
        if let Some(retry_id) =
            maybe_auto_recover_missing_delivery(self.store, self.task_id, self.args).await?
        {
            return Ok(ControlFlow::Break(Some(retry_id)));
        }
        Ok(ControlFlow::Continue(()))
    }
    pub(super) async fn finish(&mut self) -> Result<Option<TaskId>> {
        crate::task::verify::enforce_verify_status(self.store, self.task_id);
        if let Some(retry_id) = retry_or_cascade(self.store, self.args, self.task_id).await? {
            return Ok(Some(retry_id));
        }
        if let Some(task) = self.store.get_task(self.task_id.as_str())?
            && task.status == TaskStatus::Failed
            && !self.store.is_caller_owned(self.task_id.as_str())?
            && self.args.cascade.is_empty()
            && self.quota_error_message.is_some()
        {
            if let Some(retry_id) =
                quota_continuation::continue_quota(self.store, self.args, &task).await?
            {
                return Ok(Some(retry_id));
            }
        }
        if self.mode.is_foreground() && !self.args.background {
            aid_info!("[aid] View in TUI: aid board");
        }
        Ok(None)
    }
}

#[cfg(test)]
#[path = "phases_finish_tests.rs"]
mod tests;
