// Streaming watcher run state and ordered phases.
// Depends on watcher parsing, process monitoring, and persistent events.
use super::*;
impl WatchRun<'_> {
    pub(in crate::runtime::watcher) async fn stream(
        &mut self,
        prompt: Option<&str>,
        workgroup_id: Option<&str>,
        idle_timeout: Duration,
        max_task_cost: Option<f64>,
    ) -> Result<()> {
        self.stderr_handle = spawn_stderr_capture(
            self.child,
            self.task_id,
            self.watchdog.as_ref(),
            self.transcript.take(),
        );
        self.input = crate::runtime::claude_input::Session::start(
            self.child,
            self.task_id,
            self.store,
            prompt,
        )
        .await?;
        loop {
            let line = match self.next_line(idle_timeout).await? {
                ControlFlow::Continue(line) => line,
                ControlFlow::Break(()) => break,
            };
            if self.observe(&line).await?.is_break() {
                break;
            }
            if self
                .record_event(&line, workgroup_id, max_task_cost)
                .await?
                .is_break()
            {
                break;
            }
            if let Some(watchdog) = &mut self.watchdog {
                watchdog.observe(&line, self.event_count);
            }
        }
        Ok(())
    }
    async fn next_line(&mut self, idle_timeout: Duration) -> Result<ControlFlow<(), String>> {
        let line = match input::next_line(
            &mut self.input,
            self.store,
            self.task_id,
            self.watchdog.as_mut(),
            &mut self.lines,
            self.task_id.as_str(),
            self.child.id(),
            idle_timeout,
            self.last_event_detail.is_some(),
        )
        .await
        {
            Some(Ok(Some(line))) => line,
            Some(Ok(None)) => return Ok(ControlFlow::Break(())),
            Some(Err(e)) => return Err(e.into()),
            None => {
                let _ = crate::runtime::process::group::terminate_child_group(self.child).await;
                let _ = monitor::insert_hung_detected_events(
                    self.store.as_ref(),
                    self.task_id,
                    self.watchdog
                        .as_ref()
                        .map_or(idle_timeout, |watchdog| watchdog.timeout())
                        .as_secs(),
                    self.event_count,
                    self.last_event_detail.as_deref(),
                    self.watchdog
                        .as_ref()
                        .is_some_and(|watchdog| watchdog.first_token()),
                );
                self.info.status = TaskStatus::Failed;
                return Ok(ControlFlow::Break(()));
            }
        };
        Ok(ControlFlow::Continue(line))
    }
    async fn observe(&mut self, line: &str) -> Result<ControlFlow<()>> {
        if api_route::stop_if_misbilled(
            self.agent,
            line,
            self.child,
            self.store,
            self.task_id,
            &mut self.info,
        )
        .await?
        {
            self.api_route_failed = true;
            return Ok(ControlFlow::Break(()));
        }
        if let Some(input) = &mut self.input {
            input.observe(self.store, self.task_id, line)?;
        }
        if !line.trim().is_empty() {
            self.last_event_detail = Some(line.trim().to_string());
        }
        if self.agent.kind() == AgentKind::Codex {
            self.delivery_evidence.observe_codex_jsonl(line);
        }

        if !is_standalone_milestone_line(line) && !is_thinking_delta(line) {
            self.log_file.write_all(line.as_bytes()).await?;
            self.log_file.write_all(b"\n").await?;
            self.full_output.push_str(line);
            self.full_output.push('\n');
        }

        Ok(ControlFlow::Continue(()))
    }
    async fn record_event(
        &mut self,
        line: &str,
        workgroup_id: Option<&str>,
        max_task_cost: Option<f64>,
    ) -> Result<ControlFlow<()>> {
        if let Some(event_detail) = handle_streaming_line_with_session(
            StreamLineContext {
                agent: self.agent,
                task_id: self.task_id,
                store: self.store,
                workgroup_id,
                synthetic_tracker: &mut self.synthetic_tracker,
            },
            &mut self.info,
            &mut self.event_count,
            line,
            &mut self.session_saved,
        )? {
            if event_detail.kind == EventKind::Completion {
                self.saw_completion_event = true;
            }
            let detail = event_detail.detail;
            self.last_event_detail = Some(detail.clone());
            return self.check_cost(max_task_cost).await;
        }
        Ok(ControlFlow::Continue(()))
    }
    async fn check_cost(&mut self, max_task_cost: Option<f64>) -> Result<ControlFlow<()>> {
        if exceeds_cost_ceiling(self.info.cost_usd, max_task_cost) {
            let current_cost = self.info.cost_usd.unwrap_or_default();
            let max_cost = max_task_cost.unwrap_or_default();
            let _ = self.store.insert_agent_event(&TaskEvent::new(
                self.task_id.clone(),
                EventKind::Error,
                format!(
                    "Task killed: cost ${:.2} exceeded ceiling ${:.2}",
                    current_cost, max_cost
                ),
            ));
            force_kill_process_group(self.child);
            let _ = self.child.kill().await;
            self.info.status = TaskStatus::Failed;
            return Ok(ControlFlow::Break(()));
        }
        Ok(ControlFlow::Continue(()))
    }
}
