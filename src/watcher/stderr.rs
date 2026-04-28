// Stderr capture support for watcher-managed child processes.
// Exports spawn_stderr_capture and drain_stderr_capture.
// Deps: paths, TaskId, and Tokio async IO/task primitives.

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

use crate::paths;
use crate::types::TaskId;

const STDERR_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn spawn_stderr_capture(child: &mut Child, task_id: &TaskId) -> Option<JoinHandle<()>> {
    let stderr = child.stderr.take()?;
    let stderr_path = paths::stderr_path(task_id.as_str());
    Some(tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        let mut collected = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            collected.push_str(&line);
            collected.push('\n');
        }
        if !collected.is_empty() {
            let _ = tokio::fs::write(&stderr_path, &collected).await;
        }
    }))
}

pub(super) async fn drain_stderr_capture(mut handle: JoinHandle<()>) {
    if timeout(STDERR_DRAIN_TIMEOUT, &mut handle).await.is_err() {
        handle.abort();
        let _ = handle.await;
    }
}

#[cfg(test)]
mod tests {
    use super::drain_stderr_capture;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn drain_stderr_capture_times_out_stuck_handle() {
        let handle = tokio::spawn(async {
            std::future::pending::<()>().await;
        });

        let result = timeout(Duration::from_secs(3), drain_stderr_capture(handle)).await;

        assert!(result.is_ok());
    }
}
