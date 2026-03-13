// PTY monitoring helpers for interactive background tasks.
// Owns chunk parsing, prompt detection, input forwarding, and completion finalization.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::io::Write;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::agent::Agent;
use crate::input_signal;
use crate::prompt::PromptDetector;
use crate::pty_bridge::PtyBridge;
use crate::store::Store;
use crate::types::{CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};
use crate::watcher;

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) struct MonitorState {
    pub(crate) info: CompletionInfo,
    full_output: String,
    line_buffer: String,
    event_count: u32,
    prompt_detector: PromptDetector,
    awaiting_input: bool,
}

impl MonitorState {
    pub(crate) fn new() -> Self {
        Self {
            info: CompletionInfo {
                tokens: None,
                status: TaskStatus::Done,
                model: None,
                cost_usd: None,
            },
            full_output: String::new(),
            line_buffer: String::new(),
            event_count: 0,
            prompt_detector: PromptDetector::default(),
            awaiting_input: false,
        }
    }

    fn handle_chunk(
        &mut self,
        agent: &dyn Agent,
        task_id: &TaskId,
        store: &Arc<Store>,
        log_file: &mut std::fs::File,
        chunk: String,
        streaming: bool,
    ) -> Result<()> {
        log_file.write_all(chunk.as_bytes())?;
        self.full_output.push_str(&chunk);
        if streaming {
            self.line_buffer.push_str(&chunk);
            flush_stream_lines(
                agent,
                task_id,
                store,
                &mut self.info,
                &mut self.event_count,
                &mut self.line_buffer,
            )?;
        }
        if let Some(prompt) = self.prompt_detector.push_chunk(&chunk, Instant::now()) {
            let awaiting_prompt = extract_awaiting_prompt(&self.full_output, &prompt);
            mark_awaiting_input(store, task_id, &prompt, &awaiting_prompt, &mut self.awaiting_input)?;
        }
        Ok(())
    }

    fn handle_timeout(&mut self, store: &Arc<Store>, task_id: &TaskId) -> Result<()> {
        if let Some(prompt) = self.prompt_detector.poll_idle(Instant::now()) {
            let awaiting_prompt = extract_awaiting_prompt(&self.full_output, &prompt);
            mark_awaiting_input(store, task_id, &prompt, &awaiting_prompt, &mut self.awaiting_input)?;
        }
        Ok(())
    }

    fn maybe_forward_input(
        &mut self,
        bridge: &mut PtyBridge,
        store: &Arc<Store>,
        task_id: &TaskId,
    ) -> Result<()> {
        if !self.awaiting_input {
            return Ok(());
        }
        let Some(input) = input_signal::take_response(task_id.as_str())? else {
            return Ok(());
        };
        bridge.write_input(&input)?;
        store.update_task_status(task_id.as_str(), TaskStatus::Running)?;
        self.awaiting_input = false;
        self.prompt_detector.reset_after_input();
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn monitor_bridge(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    bridge: &mut PtyBridge,
    rx: &mpsc::Receiver<Vec<u8>>,
    log_file: &mut std::fs::File,
    state: &mut MonitorState,
    streaming: bool,
) -> Result<()> {
    let mut reader_done = false;
    while !reader_done || bridge.is_alive() {
        match rx.recv_timeout(INPUT_POLL_INTERVAL) {
            Ok(bytes) => {
                let chunk = String::from_utf8_lossy(&bytes).into_owned();
                state.handle_chunk(agent, task_id, store, log_file, chunk, streaming)?;
            }
            Err(RecvTimeoutError::Timeout) => state.handle_timeout(store, task_id)?,
            Err(RecvTimeoutError::Disconnected) => reader_done = true,
        }
        state.maybe_forward_input(bridge, store, task_id)?;
    }

    if streaming && !state.line_buffer.trim().is_empty() {
        watcher::handle_streaming_line(
            agent,
            task_id,
            store,
            &mut state.info,
            &mut state.event_count,
            state.line_buffer.trim_end_matches(['\r', '\n']),
        )?;
    }
    Ok(())
}

pub(crate) fn finalize_output(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    output_path: Option<&str>,
    streaming: bool,
    exit_status: &portable_pty::ExitStatus,
    state: &mut MonitorState,
) -> Result<()> {
    if streaming {
        finalize_streaming(task_id, store, exit_status, state)
    } else {
        finalize_buffered(agent, task_id, store, output_path, exit_status, state)
    }
}

fn finalize_streaming(
    task_id: &TaskId,
    store: &Arc<Store>,
    exit_status: &portable_pty::ExitStatus,
    state: &mut MonitorState,
) -> Result<()> {
    let status = if exit_status.success() {
        TaskStatus::Done
    } else {
        TaskStatus::Failed
    };
    state.info.status = status;
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: if status == TaskStatus::Done {
            EventKind::Completion
        } else {
            EventKind::Error
        },
        detail: format!(
            "{} — {} events, exit code {}",
            status.label(),
            state.event_count,
            exit_status.exit_code()
        ),
        metadata: None,
    })?;
    Ok(())
}

fn finalize_buffered(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    output_path: Option<&str>,
    exit_status: &portable_pty::ExitStatus,
    state: &mut MonitorState,
) -> Result<()> {
    if let Some(path) = output_path {
        write_output_file(path, &state.full_output)?;
    }
    state.info = if exit_status.success() {
        agent.parse_completion(&state.full_output)
    } else {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Failed,
            model: None,
            cost_usd: None,
        }
    };
    store.insert_event(&crate::agent::gemini::make_completion_event(task_id, &state.info))?;
    Ok(())
}

fn flush_stream_lines(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    info: &mut CompletionInfo,
    event_count: &mut u32,
    line_buffer: &mut String,
) -> Result<()> {
    while let Some(pos) = line_buffer.find('\n') {
        let line = line_buffer[..pos].trim_end_matches('\r').to_string();
        watcher::handle_streaming_line(agent, task_id, store, info, event_count, &line)?;
        line_buffer.drain(..=pos);
    }
    Ok(())
}

fn extract_awaiting_prompt(output: &str, prompt: &str) -> String {
    let prompt = prompt.trim();
    output
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(6)
        .find(|line| line.ends_with('?') || line.ends_with("(y/n)"))
        .unwrap_or(prompt)
        .to_string()
}

fn mark_awaiting_input(
    store: &Arc<Store>,
    task_id: &TaskId,
    prompt: &str,
    awaiting_prompt: &str,
    awaiting_input: &mut bool,
) -> Result<()> {
    if *awaiting_input {
        return Ok(());
    }
    store.update_task_status(task_id.as_str(), TaskStatus::AwaitingInput)?;
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

fn write_output_file(path: &str, buffer: &str) -> Result<()> {
    if let Some(response) = crate::agent::gemini::extract_response(buffer) {
        std::fs::write(path, response)?;
    } else {
        std::fs::write(path, buffer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
