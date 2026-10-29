// Watcher engine: reads agent stdout/stderr and records events to store.
// Exports streaming and buffered watchers plus shared watcher state.
mod buffered;
mod completion;
use completion::{apply_completion_event, exceeds_cost_ceiling};
mod input;
#[cfg(test)]
pub use input::watch_streaming;
mod api_route;
mod esc;
mod extract;
mod idle;
#[cfg(test)]
pub(crate) mod idle_tests;
#[cfg(test)]
mod opencode_usage_tests;
mod progress;
mod stderr;
mod stream;
#[cfg(test)]
mod streaming_completion_tests;
#[cfg(test)]
mod streaming_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transcript_tests;

use crate::agent::Agent;
use crate::routing::rate_limit;
use crate::runtime::process::group::force_kill_process_group;
use crate::runtime::process::monitor;
use crate::store::Store;
use crate::task::delivery_guard::{DeliveryEvidence, DeliveryOutcome};
use crate::types::*;
use anyhow::Result;
pub(crate) use buffered::watch_buffered;
pub(crate) use esc::strip_terminal_escapes;
#[cfg(test)]
use extract::{extract_finding_detail, extract_milestone_detail, parse_milestone_event};
use extract::{is_standalone_milestone_line, is_thinking_delta};
pub(crate) use progress::SyntheticMilestoneTracker;
use std::sync::Arc;
pub(crate) use stderr::read_capped_stderr;
use stderr::{
    append_stderr_to_log, drain_stderr_capture, failure_stderr_note, spawn_stderr_capture,
};
pub(crate) use stream::{
    StreamLineContext, apply_codex_delivery_guard, handle_streaming_line_with_session,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::time::Duration;
mod watch_streaming_with_input;
use watch_streaming_with_input::WatchRun;

/// Watch a child process, parse output, store events, return completion info
pub(crate) async fn watch_streaming_with_input(
    agent: &dyn Agent,
    child: &mut Child,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
    workgroup_id: Option<&str>,
    idle_timeout: Duration,
    max_task_cost: Option<f64>,
    prompt: Option<&str>,
    policy: Option<crate::config::timeout_policy::TimeoutPolicy>,
) -> Result<CompletionInfo> {
    let mut run = WatchRun::setup(agent, child, task_id, store, log_path, prompt, policy).await?;
    run.stream(prompt, workgroup_id, idle_timeout, max_task_cost)
        .await?;
    run.finish().await
}
