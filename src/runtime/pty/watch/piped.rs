// Piped Claude liveness using the PTY first-token, idle and escalation policy.
// Tracks raw reads separately from parsed progress and shares descendant activity probes.
use super::*;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};

#[derive(Default)]
struct Activity {
    raw: Option<Instant>,
    progress: Option<Instant>,
}

pub(crate) struct Watchdog {
    state: MonitorState,
    raw: Arc<Mutex<Activity>>,
    policy: crate::config::timeout_policy::TimeoutPolicy,
    first_token: bool,
    last_activity_probe: Option<Instant>,
    child_activity: Option<std::time::SystemTime>,
}

impl Watchdog {
    pub(crate) fn new(policy: crate::config::timeout_policy::TimeoutPolicy) -> Self {
        Self {
            state: MonitorState::with_policy(true, None, policy),
            raw: Arc::new(Mutex::new(Activity::default())),
            policy,
            first_token: false,
            last_activity_probe: None,
            child_activity: None,
        }
    }

    pub(crate) fn observe(&mut self, line: &str, count: u32) {
        self.state.event_count = count;
        // Replayed operator input acknowledges delivery, not agent progress.
        let replay = serde_json::from_str::<serde_json::Value>(line)
            .is_ok_and(|value| value["type"] == "user" && value["isReplay"] == true);
        if !replay && is_agent_output(line) {
            self.state.mark_progress();
            self.state.last_event_detail = Some(line.to_string());
        }
    }

    pub(crate) fn first_token(&self) -> bool {
        self.first_token
    }

    pub(crate) fn timeout(&self) -> Duration {
        if self.first_token {
            self.policy.first_token
        } else {
            self.policy.idle
        }
    }

    pub(crate) fn tick(
        &mut self,
        store: &Arc<Store>,
        task: &TaskId,
        pid: Option<u32>,
    ) -> Result<bool> {
        self.tick_with_probe(
            store,
            task,
            pid,
            Instant::now(),
            process::monitor::activity::latest,
        )
    }

    fn tick_with_probe(
        &mut self,
        store: &Arc<Store>,
        task: &TaskId,
        pid: Option<u32>,
        now: Instant,
        probe: impl FnOnce(&str, Option<u32>, Duration) -> Option<std::time::SystemTime>,
    ) -> Result<bool> {
        {
            let activity = self
                .raw
                .lock()
                .map_err(|_| anyhow::anyhow!("Raw read clock poisoned"))?;
            if let Some(raw) = activity.raw {
                self.state.received_raw_bytes = true;
                self.state.last_raw_chunk_time = raw;
            }
            if let Some(progress) = activity
                .progress
                .filter(|time| *time > self.state.last_progress_time)
            {
                self.state.last_progress_time = progress;
            }
        }
        self.refresh_descendant_activity(task, pid, now, probe);
        self.first_token =
            self.state
                .first_token_hang_elapsed(true, self.policy.first_token, task.as_str())
                && !self.child_activity.is_some_and(|activity| {
                    activity.elapsed().unwrap_or_default() <= self.policy.first_token
                });
        if self.first_token {
            return Ok(true);
        }
        if self
            .state
            .idle_hang_elapsed(true, self.policy.idle, task.as_str())
        {
            return Ok(true);
        }
        self.state.maybe_handle_idle(store, task, false)?;
        Ok(false)
    }

    fn refresh_descendant_activity(
        &mut self,
        task: &TaskId,
        pid: Option<u32>,
        now: Instant,
        probe: impl FnOnce(&str, Option<u32>, Duration) -> Option<std::time::SystemTime>,
    ) {
        let cadence = (self.policy.idle / 4).min(Duration::from_millis(500));
        if (!self.state.received_raw_bytes && self.state.progress_count() > 1)
            || self
                .last_activity_probe
                .is_some_and(|last| now.saturating_duration_since(last) < cadence)
        {
            return;
        }
        self.last_activity_probe = Some(now);
        let timeout = if self.state.progress_count() <= 1 {
            self.policy.first_token
        } else {
            self.policy.idle
        };
        self.child_activity = probe(task.as_str(), pid, timeout);
        if let Some(activity) = self.child_activity {
            let age = activity.elapsed().unwrap_or_default();
            if age < self.state.last_progress_time.elapsed() {
                self.state.last_progress_time = Instant::now() - age;
                self.state.idle_warned = false;
                self.state.idle_nudged = false;
            }
        }
    }
}

pub(crate) struct Reader<R> {
    inner: R,
    raw: Option<Arc<Mutex<Activity>>>,
    transcript: Option<Arc<Mutex<std::fs::File>>>,
    stderr: bool,
}

impl<R> Reader<R> {
    pub(crate) fn new(
        inner: R,
        watchdog: Option<&Watchdog>,
        transcript: Option<Arc<Mutex<std::fs::File>>>,
        stderr: bool,
    ) -> Self {
        Self {
            inner,
            raw: watchdog.map(|watchdog| watchdog.raw.clone()),
            transcript,
            stderr,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Reader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        if buf.filled().len() > before {
            if let Some(raw) = &self.raw {
                if let Ok(mut activity) = raw.lock() {
                    activity.raw = Some(Instant::now());
                    if self.stderr
                        && buf.filled()[before..]
                            .iter()
                            .any(|byte| !byte.is_ascii_whitespace())
                    {
                        activity.progress = activity.raw;
                    }
                }
            }
        }
        if let Some(transcript) = &self.transcript {
            if let Ok(mut file) = transcript.lock() {
                if let Err(error) = file.write_all(&buf.filled()[before..]) {
                    return Poll::Ready(Err(error));
                }
            }
        }
        result
    }
}

#[cfg(test)]
#[path = "piped_tests.rs"]
mod tests;
