// Streaming reads with the shared descendant CPU/remote-wait activity probe.
// Keeps silence before the first output on the existing output-only timeout.
// Deps: Tokio lines and timeout, monitor::activity, std time.

use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, Lines};

pub(super) async fn next_line<R: AsyncBufRead + Unpin>(
    lines: &mut Lines<R>,
    task_id: &str,
    pid: Option<u32>,
    idle: Duration,
    saw_output: bool,
) -> Option<std::io::Result<Option<String>>> {
    let mut clock = IdleClock::new();
    let poll = (idle / 4)
        .min(Duration::from_millis(500))
        .max(Duration::from_millis(1));
    loop {
        if saw_output {
            let activity = crate::runtime::process::monitor::activity::latest(task_id, pid, idle);
            clock.observe(
                Instant::now(),
                activity.map(|time| time.elapsed().unwrap_or_default()),
            );
        }
        let remaining = idle.saturating_sub(clock.last.elapsed());
        if remaining.is_zero() {
            return None;
        }
        if let Ok(result) = tokio::time::timeout(poll.min(remaining), lines.next_line()).await {
            return Some(result);
        }
    }
}

struct IdleClock {
    last: Instant,
    sampled: bool,
}

impl IdleClock {
    fn new() -> Self {
        Self {
            last: Instant::now(),
            sampled: false,
        }
    }

    fn observe(&mut self, now: Instant, activity: Option<Duration>) {
        if !self.sampled {
            self.last = now;
            self.sampled = true;
        }
        if let Some(age) = activity {
            if let Some(progress) = now.checked_sub(age) {
                self.last = self.last.max(progress);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_initial_probe_gets_a_full_idle_window_to_observe_growth() {
        let mut clock = IdleClock::new();
        let now = clock.last + Duration::from_secs(2);
        clock.observe(now, None);
        assert_eq!(clock.last, now);
        clock.observe(now + Duration::from_millis(100), None);
        assert_eq!(clock.last, now);
    }

    #[test]
    fn missing_or_stale_probe_preserves_last_observed_activity() {
        let mut clock = IdleClock::new();
        let start = clock.last;
        clock.observe(start, None);
        let progress = start + Duration::from_millis(300);
        clock.observe(progress, Some(Duration::ZERO));
        let last = clock.last;
        assert!(last > start);
        clock.observe(progress + Duration::from_millis(200), None);
        assert_eq!(clock.last, last);
        clock.observe(
            progress + Duration::from_millis(300),
            Some(Duration::from_secs(10)),
        );
        assert_eq!(clock.last, last);
    }
}
