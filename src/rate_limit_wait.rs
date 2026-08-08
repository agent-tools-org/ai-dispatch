// Async declared-urgency wait policy for known agent rate limits.
// Exports: wait_for_declared_reset().
// Deps: rate-limit markers, task-profile persistence, tokio time.

use anyhow::Result;

use crate::store::Store;
use crate::types::{AgentKind, TaskUrgency};

const POLL_INTERVAL_SECS: u64 = 5;

pub(crate) async fn wait_for_declared_reset(
    store: &Store,
    task_id: &str,
    agent: AgentKind,
    custom_name: Option<&str>,
) -> Result<()> {
    let profile = store.get_task_profile(task_id)?;
    if profile.urgency != Some(TaskUrgency::Background) {
        return Ok(());
    }
    while crate::rate_limit::is_rate_limited(&agent, custom_name) {
        tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
    Ok(())
}
