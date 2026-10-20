// Streaming watcher entry point and input polling alongside idle detection.
// Keeps the existing watcher API for transports without streaming stdin.
use super::*;

pub async fn watch_streaming(
    agent: &dyn Agent,
    child: &mut Child,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
    workgroup_id: Option<&str>,
    idle_timeout: Duration,
    max_task_cost: Option<f64>,
) -> Result<CompletionInfo> {
    watch_streaming_with_input(
        agent,
        child,
        task_id,
        store,
        log_path,
        workgroup_id,
        idle_timeout,
        max_task_cost,
        None,
        None,
    )
    .await
}

pub(super) async fn next_line<R: tokio::io::AsyncBufRead + Unpin>(
    input: &mut Option<crate::runtime::claude_input::Session>,
    store: &Arc<Store>,
    task: &TaskId,
    mut watchdog: Option<&mut crate::runtime::pty::watch::piped::Watchdog>,
    lines: &mut tokio::io::Lines<R>,
    task_id: &str,
    pid: Option<u32>,
    idle_timeout: Duration,
    saw_output: bool,
) -> Option<std::io::Result<Option<String>>> {
    let monitored = watchdog.is_some();
    let read = async {
        if monitored {
            Some(lines.next_line().await)
        } else {
            idle::next_line(lines, task_id, pid, idle_timeout, saw_output).await
        }
    };
    tokio::pin!(read);
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            result = &mut read => return result,
            _ = tick.tick(), if input.is_some() || monitored => {
                if let Some(watchdog) = &mut watchdog {
                    match watchdog.tick(store, task, pid) {
                        Ok(true) => return None,
                        Ok(false) => {},
                        Err(error) => return Some(Err(std::io::Error::other(error.to_string()))),
                    }
                }
                if let Some(input) = input {
                    if let Err(error) = input.poll(store, task).await {
                        return Some(Err(std::io::Error::other(error.to_string())));
                    }
                }
            }
        }
    }
}
