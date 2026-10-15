// PTY output helpers moved from the shared monitor.
// Deps: parent monitor state, agent parsing, Store and PTY control.

use super::*;

impl MonitorState {
    pub(super) fn handle_chunk(
        &mut self,
        agent: &dyn Agent,
        task_id: &TaskId,
        store: &Arc<Store>,
        log_file: &mut std::fs::File,
        chunk: String,
    ) -> Result<()> {
        let chunk = watcher::strip_terminal_escapes(&chunk);
        log_file.write_all(chunk.as_bytes())?;
        self.full_output.push_str(&chunk);
        self.line_buffer.push_str(&chunk);
        self.flush_output_lines(agent, task_id, store)?;
        if !self.streaming
            && let Some(prompt) = self.prompt_detector.push_chunk(&chunk, Instant::now())
        {
            self.mark_prompt(store, task_id, &prompt)?;
        }
        Ok(())
    }

    pub(super) fn flush_output_lines(
        &mut self,
        agent: &dyn Agent,
        task_id: &TaskId,
        store: &Arc<Store>,
    ) -> Result<()> {
        while let Some(pos) = self.line_buffer.find('\n') {
            let line = self.line_buffer[..pos].trim_end_matches('\r').to_string();
            let is_echo = take_inbound_echo(&mut self.inbound_echo_suppress, &line);
            if !is_echo && agent.kind() == AgentKind::Codex {
                self.delivery_evidence.observe_codex_jsonl(&line);
            }
            if !is_echo && is_agent_output(&line) {
                self.mark_progress();
            }
            if self.streaming && !is_echo {
                if let Some(event_detail) = watcher::handle_streaming_line_with_session(
                    watcher::StreamLineContext {
                        agent,
                        task_id,
                        store,
                        workgroup_id: self.workgroup_id.as_deref(),
                        synthetic_tracker: &mut self.synthetic_tracker,
                    },
                    &mut self.info,
                    &mut self.event_count,
                    &line,
                    &mut self.session_saved,
                )? {
                    if event_detail.kind == EventKind::Completion {
                        self.saw_completion_event = true;
                    }
                    if event_detail.kind.is_liveness() {
                        self.last_event_detail = Some(event_detail.detail);
                    }
                }
            }
            self.line_buffer.drain(..=pos);
        }
        Ok(())
    }

    pub(super) fn flush_trailing_output(
        &mut self,
        agent: &dyn Agent,
        task_id: &TaskId,
        store: &Arc<Store>,
    ) -> Result<()> {
        let trailing = self.line_buffer.trim_end_matches(['\r', '\n']).to_string();
        if trailing.trim().is_empty() {
            return Ok(());
        }
        let is_echo = take_inbound_echo(&mut self.inbound_echo_suppress, &trailing);
        if !is_echo && agent.kind() == AgentKind::Codex {
            self.delivery_evidence.observe_codex_jsonl(&trailing);
        }
        if !is_echo && is_agent_output(&trailing) {
            self.mark_progress();
        }
        if self.streaming && !is_echo {
            if let Some(event_detail) = watcher::handle_streaming_line_with_session(
                watcher::StreamLineContext {
                    agent,
                    task_id,
                    store,
                    workgroup_id: self.workgroup_id.as_deref(),
                    synthetic_tracker: &mut self.synthetic_tracker,
                },
                &mut self.info,
                &mut self.event_count,
                &trailing,
                &mut self.session_saved,
            )? {
                if event_detail.kind == EventKind::Completion {
                    self.saw_completion_event = true;
                }
                if event_detail.kind.is_liveness() {
                    self.mark_progress();
                    self.last_event_detail = Some(event_detail.detail);
                }
            }
        }
        self.line_buffer.clear();
        Ok(())
    }
}

pub(super) fn extract_awaiting_prompt(output: &str, prompt: &str) -> String {
    let prompt = prompt.trim();
    let cleaned = crate::runtime::watcher::strip_terminal_escapes(output);
    let lines: Vec<&str> = cleaned
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(20)
        .collect();

    let question_match = lines.iter().find(|line| line.ends_with('?'));
    if let Some(q) = question_match {
        return q.to_string();
    }

    let patterns = [
        "(y/n)",
        "(Y/n)",
        "(yes/no)",
        "(Yes/No)",
        "Do you want",
        "Would you like",
        "Shall I",
        "Should I",
        "Please confirm",
        "Continue?",
    ];
    for line in &lines {
        if line.starts_with('>') || line.starts_with('?') {
            return line.to_string();
        }
        for pattern in &patterns {
            if line.contains(pattern) {
                return line.to_string();
            }
        }
    }

    prompt.to_string()
}

pub(super) fn mark_awaiting_input(
    store: &Arc<Store>,
    task_id: &TaskId,
    prompt: &str,
    awaiting_prompt: &str,
    awaiting_input: &mut bool,
) -> Result<()> {
    if *awaiting_input {
        return Ok(());
    }
    crate::task_lifecycle::mark_awaiting_input(store.as_ref(), task_id)?;
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Reasoning,
        detail: prompt.to_string(),
        metadata: Some(json!({ "awaiting_input": true, "awaiting_prompt": awaiting_prompt })),
    })?;
    *awaiting_input = true;
    Ok(())
}

pub(super) fn write_output_file(agent: AgentKind, path: &str, buffer: &str) -> Result<()> {
    if let Some(response) = crate::agent::extract_response(agent, buffer) {
        std::fs::write(path, response)?;
    } else {
        std::fs::write(path, buffer)?;
    }
    Ok(())
}

pub(super) fn persist_transcript(task_id: &TaskId, buffer: &str) {
    let _ = std::fs::create_dir_all(crate::support::paths::task_dir(task_id.as_str()));
    let _ = std::fs::write(crate::support::paths::transcript_path(task_id.as_str()), buffer);
}
