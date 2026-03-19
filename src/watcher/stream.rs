// Watcher stream handlers for per-line event processing.
// Exports shared streaming helpers used by child-process and PTY watchers.

use anyhow::Result;
use std::sync::Arc;

use crate::agent::Agent;
use crate::rate_limit;
use crate::store::Store;
use crate::types::{CompletionInfo, TaskId};

use super::extract::{append_to_broadcast, extract_finding_detail, parse_milestone_event};
use super::{apply_completion_event, SyntheticMilestoneTracker};

pub(crate) struct StreamLineContext<'a> {
    pub agent: &'a dyn Agent,
    pub task_id: &'a TaskId,
    pub store: &'a Arc<Store>,
    pub workgroup_id: Option<&'a str>,
    pub synthetic_tracker: &'a mut SyntheticMilestoneTracker,
}

pub(crate) fn handle_streaming_line(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    info: &mut CompletionInfo,
    event_count: &mut u32,
    synthetic_tracker: &mut SyntheticMilestoneTracker,
    workgroup_id: Option<&str>,
    line: &str,
) -> Result<()> {
    let ctx = StreamLineContext {
        agent,
        task_id,
        store,
        workgroup_id,
        synthetic_tracker,
    };
    let _ = handle_streaming_line_with_session(ctx, info, event_count, line, &mut false)?;
    Ok(())
}

pub(crate) fn handle_streaming_line_with_session(
    ctx: StreamLineContext<'_>,
    info: &mut CompletionInfo,
    event_count: &mut u32,
    line: &str,
    session_saved: &mut bool,
) -> Result<Option<String>> {
    let StreamLineContext {
        agent,
        task_id,
        store,
        workgroup_id,
        synthetic_tracker,
    } = ctx;

    if let Some(finding) = extract_finding_detail(line)
        && let Some(group_id) = workgroup_id
    {
        let _ = store.insert_finding(
            group_id,
            &finding,
            Some(task_id.as_str()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        append_to_broadcast(group_id, task_id.as_str(), &finding);
    }

    if let Some(event) = parse_milestone_event(task_id, line) {
        synthetic_tracker.observe(&event);
        store.insert_event(&event)?;
        *event_count += 1;
        return Ok(Some(event.detail.clone()));
    }

    if let Some(event) = agent.parse_event(task_id, line) {
        apply_completion_event(info, &event);
        synthetic_tracker.observe(&event);
        save_session_id(store, task_id, &event, session_saved)?;
        if rate_limit::is_rate_limit_error(&event.detail) {
            rate_limit::mark_rate_limited(&agent.kind(), &event.detail);
        }
        store.insert_event(&event)?;
        *event_count += 1;
        if let Some(event) = synthetic_tracker.synthetic_event(task_id, &event) {
            store.insert_event(&event)?;
            *event_count += 1;
        }
        return Ok(Some(event.detail.clone()));
    }

    Ok(None)
}

fn save_session_id(
    store: &Arc<Store>,
    task_id: &TaskId,
    event: &crate::types::TaskEvent,
    session_saved: &mut bool,
) -> Result<()> {
    if *session_saved {
        return Ok(());
    }
    let Some(metadata) = &event.metadata else {
        return Ok(());
    };
    let Some(session_id) = metadata.get("agent_session_id").and_then(|s| s.as_str()) else {
        return Ok(());
    };
    store.update_agent_session_id(task_id.as_str(), session_id)?;
    *session_saved = true;
    Ok(())
}
