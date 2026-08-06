// Stdout/stderr stream pumping and cargo JSON/libtest line classification.
// Exports: StreamEvent, CargoStreamState, pump/drain/handle helpers.
// Deps: tokio io, DiagnosticCollector, permission text, Store events.

use super::build_diag::DiagnosticCollector;
use super::build_fallback::is_permission_os_error_text;
use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::mpsc;

#[derive(Debug)]
pub(super) enum StreamEvent {
    Stdout(String),
    Stderr(String),
    Done,
}

#[derive(Debug, Default)]
pub(super) struct CargoStreamState {
    pub(super) collector: DiagnosticCollector,
    pub(super) stderr_lines: Vec<String>,
    pub(super) plain_stdout: Vec<String>,
    pub(super) compiled_units: usize,
    pub(super) done_streams: usize,
}

pub(super) async fn pump_lines<R, F>(reader: R, tx: mpsc::Sender<StreamEvent>, build_event: F)
where
    R: AsyncRead + Unpin,
    F: Fn(String) -> StreamEvent + Copy,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if tx.send(build_event(line)).await.is_err() {
            return;
        }
    }
    let _ = tx.send(StreamEvent::Done).await;
}

pub(super) async fn drain_streams(
    rx: &mut mpsc::Receiver<StreamEvent>,
    store: &Store,
    task_id: &Option<String>,
    stream_state: &mut CargoStreamState,
) {
    while stream_state.done_streams < 2 {
        let event = rx.recv().await;
        handle_stream_event(event, store, task_id, stream_state);
    }
}

pub(super) fn handle_stream_event(
    event: Option<StreamEvent>,
    store: &Store,
    task_id: &Option<String>,
    stream_state: &mut CargoStreamState,
) {
    match event {
        Some(StreamEvent::Stdout(line)) => {
            if is_compiler_artifact_line(&line) {
                stream_state.compiled_units += 1;
            }
            if let Some(diagnostic) = stream_state.collector.push_json_line(&line) {
                // JSON compiler-message path: EPERM never hits stderr, but fallback
                // detection keys off the human message text.
                if is_permission_os_error_text(&diagnostic.message) {
                    stream_state.stderr_lines.push(diagnostic.message.clone());
                }
                emit_event(store, task_id, diagnostic.event_detail());
            } else if !line.trim_start().starts_with('{') {
                stream_state.plain_stdout.push(line);
            }
        }
        Some(StreamEvent::Stderr(line)) => stream_state.stderr_lines.push(line),
        Some(StreamEvent::Done) | None => stream_state.done_streams += 1,
    }
}

pub(super) fn is_compiler_artifact_line(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|value| value.get("reason").and_then(|reason| reason.as_str()).map(str::to_string))
        .as_deref()
        == Some("compiler-artifact")
}

pub(super) fn emit_event(store: &Store, task_id: &Option<String>, detail: String) {
    if let Some(task_id) = task_id.as_ref() {
        let _ = store.insert_event(&TaskEvent {
            task_id: TaskId(task_id.clone()),
            timestamp: chrono::Local::now(),
            event_kind: EventKind::Build,
            detail,
            metadata: None,
        });
    }
}
