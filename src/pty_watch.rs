// PTY monitoring helpers for interactive background tasks.
// Owns chunk parsing, prompt detection, input forwarding, and completion finalization.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::io::Write;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
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
    streaming: bool,
}

impl MonitorState {
    pub(crate) fn new(streaming: bool) -> Self {
        Self {
            info: CompletionInfo {
                tokens: None,
                status: TaskStatus::Done,
                model: None,
                cost_usd: None,
                exit_code: None,
            },
            full_output: String::new(),
            line_buffer: String::new(),
            event_count: 0,
            prompt_detector: PromptDetector::default(),
            awaiting_input: false,
            streaming,
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
        if !self.streaming
            && let Some(prompt) = self.prompt_detector.push_chunk(&chunk, Instant::now())
        {
            let awaiting_prompt = extract_awaiting_prompt(&self.full_output, &prompt);
            mark_awaiting_input(
                store,
                task_id,
                &prompt,
                &awaiting_prompt,
                &mut self.awaiting_input,
            )?;
        }
        Ok(())
    }

    fn handle_timeout(&mut self, store: &Arc<Store>, task_id: &TaskId) -> Result<()> {
        if !self.streaming
            && let Some(prompt) = self.prompt_detector.poll_idle(Instant::now())
        {
            let awaiting_prompt = extract_awaiting_prompt(&self.full_output, &prompt);
            mark_awaiting_input(
                store,
                task_id,
                &prompt,
                &awaiting_prompt,
                &mut self.awaiting_input,
            )?;
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

    fn maybe_forward_steer(
        &mut self,
        bridge: &mut PtyBridge,
        store: &Arc<Store>,
        task_id: &TaskId,
    ) -> Result<()> {
        let Some(message) = input_signal::take_steer(task_id.as_str())? else {
            return Ok(());
        };
        bridge.write_input(&message)?;
        store.insert_event(&TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: format!("Steered: {}", message.chars().take(200).collect::<String>()),
            metadata: Some(json!({ "steered": true })),
        })?;
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
        state.maybe_forward_steer(bridge, store, task_id)?;
    }

    if streaming && !state.line_buffer.trim().is_empty() {
        watcher::handle_streaming_line(
            agent,
            task_id,
            store,
            &mut state.info,
            &mut state.event_count,
            None,
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
    state.info.exit_code = i32::try_from(exit_status.exit_code()).ok();
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
            exit_code: None,
        }
    };
    state.info.exit_code = i32::try_from(exit_status.exit_code()).ok();
    store.insert_event(&crate::agent::gemini::make_completion_event(
        task_id,
        &state.info,
    ))?;
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
        watcher::handle_streaming_line(agent, task_id, store, info, event_count, None, &line)?;
        line_buffer.drain(..=pos);
    }
    Ok(())
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                i = j + 1;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn extract_awaiting_prompt(output: &str, prompt: &str) -> String {
    let prompt = prompt.trim();
    let cleaned = strip_ansi(output);
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
