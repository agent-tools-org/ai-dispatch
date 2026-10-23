// PTY bridge run state and ordered polling phases.
// Depends on monitor helpers, UTF-8 decoding, and PTY control.
use super::*;
use std::ops::ControlFlow;

pub(super) struct MonitorRun<'a> {
    pub(super) agent: &'a dyn Agent,
    pub(super) task_id: &'a TaskId,
    pub(super) store: &'a Arc<Store>,
    pub(super) bridge: &'a mut PtyBridge,
    pub(super) rx: &'a mpsc::Receiver<Vec<u8>>,
    pub(super) log_file: &'a mut std::fs::File,
    pub(super) state: &'a mut MonitorState,
    pub(super) first_token_timeout: Option<Duration>,
    pub(super) idle_timeout: Option<Duration>,
    pub(super) max_task_cost: Option<f64>,
    pub(super) reader_done: bool,
    pub(super) child_exited_at: Option<Instant>,
    pub(super) decoder: utf8::Utf8Chunks,
    pub(super) last_child_activity: std::time::SystemTime,
}
impl MonitorRun<'_> {
    pub(super) fn monitor(&mut self) -> Result<()> {
        while !drained(self.bridge, self.reader_done, &mut self.child_exited_at) {
            if deadline_elapsed(self.state, self.store, self.task_id)? {
                break;
            }
            if self.poll()?.is_break() {
                break;
            }
            forward_input(
                self.state,
                self.agent,
                self.bridge,
                self.store,
                self.task_id,
            )?;
        }
        self.state
            .flush_trailing_output(self.agent, self.task_id, self.store)
    }
    fn poll(&mut self) -> Result<ControlFlow<()>> {
        match self.rx.recv_timeout(INPUT_POLL_INTERVAL) {
            Ok(bytes) => {
                return self.handle_output(bytes);
            }
            Err(RecvTimeoutError::Timeout) => {
                if handle_idle_timeout(
                    self.state,
                    self.agent,
                    self.store,
                    self.task_id,
                    self.bridge,
                    self.first_token_timeout,
                    self.idle_timeout,
                    &mut self.last_child_activity,
                )? {
                    return Ok(ControlFlow::Break(()));
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.state.handle_chunk(
                    self.agent,
                    self.task_id,
                    self.store,
                    self.log_file,
                    self.decoder.flush(),
                )?;
                self.reader_done = true;
            }
        }
        Ok(ControlFlow::Continue(()))
    }
    fn handle_output(&mut self, bytes: Vec<u8>) -> Result<ControlFlow<()>> {
        if let Err(error) = handle_bytes(
            self.state,
            self.agent,
            self.task_id,
            self.store,
            self.log_file,
            &mut self.decoder,
            bytes,
        ) {
            let _ = self.bridge.kill_group();
            return Err(error);
        }
        if stop_after_output(
            self.state,
            self.agent,
            self.store,
            self.task_id,
            self.bridge,
            self.max_task_cost,
        ) {
            return Ok(ControlFlow::Break(()));
        }
        Ok(ControlFlow::Continue(()))
    }
}
