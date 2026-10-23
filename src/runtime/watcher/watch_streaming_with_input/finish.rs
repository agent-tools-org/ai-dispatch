// Streaming watcher run state and ordered phases.
// Depends on watcher parsing, process monitoring, and persistent events.
use super::*;
impl WatchRun<'_> {
    pub(in crate::runtime::watcher) async fn finish(mut self) -> Result<CompletionInfo> {
        self.log_file.flush().await?;
        let exit_status = self.child.wait().await?;
        if let Some(handle) = self.stderr_handle.take() {
            drain_stderr_capture(handle).await;
        }
        let mut status = if exit_status.success()
            && !self.api_route_failed
            && self.info.status != TaskStatus::Failed
        {
            TaskStatus::Done
        } else {
            TaskStatus::Failed
        };
        self.info.exit_code = exit_status.code();
        let task = self.store.get_task(self.task_id.as_str()).ok().flatten();
        let delivery_outcome = self.delivery_evidence.validate();
        let delivered = if self.agent.kind() == AgentKind::Codex {
            matches!(&delivery_outcome, DeliveryOutcome::Delivered)
        } else {
            self.saw_completion_event
        };
        if self.agent.kind() == AgentKind::Codex {
            status = apply_codex_delivery_guard(
                self.store,
                self.task_id,
                status,
                delivery_outcome,
                exit_status.code(),
            );
        }

        if status == TaskStatus::Done {
            self.info.status = status;
            let parsed = self.agent.parse_completion(&self.full_output);
            crate::agent::stream_completion::merge_parsed_completion(&mut self.info, parsed);
            status = self.info.status;
        }

        status = self.record_quota(task.as_ref(), status, delivered);
        self.record_terminal(status, exit_status, task.as_ref())
            .await
    }
    fn record_quota(
        &mut self,
        task: Option<&Task>,
        mut status: TaskStatus,
        delivered: bool,
    ) -> TaskStatus {
        // Checked before the clear below: a quota refusal arrives as exit 0 text.
        // agy and other plain-text CLIs never echo their model, so the group a
        // quota belongs to is only knowable from what aid dispatched.
        let dispatched_model = task.and_then(|t| t.requested_model.as_deref());
        let quota = crate::agent::stream_completion::record_quota_exhaustion_with_delivery(
            &self.full_output,
            self.agent.kind(),
            self.agent.rate_limit_name(),
            self.info.model.as_deref().or(dispatched_model),
            delivered,
        );
        if quota.should_fail() {
            status = TaskStatus::Failed;
            self.info.status = status;
        }

        // A run that delivered *and* hit a refusal stays Done, so this clear would
        // erase the marker written microseconds earlier and hand routing back a
        // provider that is out.
        if status == TaskStatus::Done && !quota.recorded() {
            let model = self.info.model.as_deref().or(dispatched_model);
            rate_limit::clear_rate_limit_for_model(
                &self.agent.kind(),
                self.agent.rate_limit_name(),
                model,
            );
        }
        status
    }
    async fn record_terminal(
        mut self,
        status: TaskStatus,
        exit_status: std::process::ExitStatus,
        task: Option<&Task>,
    ) -> Result<CompletionInfo> {
        let dispatched_model = task.and_then(|t| t.requested_model.as_deref());
        self.preserve_stderr(status, exit_status).await?;
        let stderr_note = failure_stderr_note(
            status,
            self.task_id,
            self.agent,
            dispatched_model.or(self.info.model.as_deref()),
        );
        let detail = format!(
            "{} — {} events, exit code {}{}",
            crate::routing::capacity::terminal_label(
                self.store,
                self.task_id,
                self.agent.kind(),
                status
            ),
            self.event_count,
            exit_status.code().unwrap_or(-1),
            stderr_note,
        );
        self.store.insert_agent_event(&TaskEvent {
            task_id: self.task_id.clone(),
            timestamp: Local::now(),
            event_kind: if status == TaskStatus::Done {
                EventKind::Completion
            } else {
                EventKind::Error
            },
            detail,
            metadata: None,
        })?;
        self.info.status = status;
        Ok(self.info)
    }
    async fn preserve_stderr(
        &mut self,
        status: TaskStatus,
        exit_status: std::process::ExitStatus,
    ) -> Result<()> {
        // A fast-fail whose only output was stderr (e.g. qwen's "No saved session
        // found with ID ...") would otherwise leave a zero-byte log and discard the
        // cause that was on screen. Preserve surviving stderr in the log so the
        // reason is not lost. This is deliberately narrow: it fires only when the
        // agent exited on its own (a non-zero exit code, not a signal) with no
        // stdout events — a hung or cost-killed process is terminated by signal and
        // its stderr is not replayed. Reads are capped so an oversized stderr is
        // not copied unbounded into the log.
        if status == TaskStatus::Failed
            && self.full_output.trim().is_empty()
            && exit_status.code().is_some()
        {
            append_stderr_to_log(&mut self.log_file, self.task_id).await?;
        }
        Ok(())
    }
}
