// Streaming reads with the shared descendant CPU/remote-wait activity probe.
// Keeps silence before the first output on the existing output-only timeout.
// Deps: Tokio lines and timeout, monitor::activity, std time.

use std::time::{Duration, SystemTime};
use tokio::io::{AsyncBufRead, Lines};

pub(super) async fn next_line<R: AsyncBufRead + Unpin>(
    lines: &mut Lines<R>,
    task_id: &str,
    pid: Option<u32>,
    idle: Duration,
    saw_output: bool,
) -> Option<std::io::Result<Option<String>>> {
    let start = SystemTime::now();
    let poll = (idle / 4)
        .min(Duration::from_secs(1))
        .max(Duration::from_millis(1));
    loop {
        let activity = crate::runtime::process::monitor::activity::latest(task_id, pid, idle);
        let last = if saw_output {
            activity.unwrap_or(start).max(start)
        } else {
            start
        };
        let remaining = idle.saturating_sub(last.elapsed().unwrap_or_default());
        if remaining.is_zero() {
            return None;
        }
        if let Ok(result) = tokio::time::timeout(poll.min(remaining), lines.next_line()).await {
            return Some(result);
        }
    }
}
