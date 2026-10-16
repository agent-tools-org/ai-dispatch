// Early API-route admission failure handling for child-process streams.
// Records only a sanitized reason, then stops the process group before later output.
use super::*;

pub(super) async fn stop_if_misbilled(
    agent: &dyn Agent,
    line: &str,
    child: &mut Child,
    store: &Store,
    task_id: &TaskId,
    info: &mut CompletionInfo,
) -> Result<bool> {
    let result = crate::agent::api_route::record_stream_failure(agent, line, store, task_id, info);
    if matches!(result, Ok(false)) {
        return Ok(false);
    }
    force_kill_process_group(child);
    let _ = child.kill().await;
    result
}

#[cfg(test)]
#[path = "api_route_tests.rs"]
mod tests;
