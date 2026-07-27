// aid CLI task operation dispatch handlers.
// Implements retry, merge, response, stop, and related task wrappers.

use super::resolve_group;
use crate::cmd;
use crate::store;
use anyhow::{Result, anyhow};
use std::sync::Arc;

pub(super) async fn retry(
    store: Arc<store::Store>,
    task_id: String,
    feedback: String,
    agent: Option<String>,
    dir: Option<String>,
    reset: bool,
    bg: bool,
) -> Result<()> {
    cmd::retry::run(store, cmd::retry::RetryArgs { task_id, feedback, agent, dir, reset, bg })
        .await
        .map(|_| ())
}

pub(super) fn merge(
    store: Arc<store::Store>,
    task_id: Option<String>,
    group: Option<String>,
    approve: bool,
    check: bool,
    force: bool,
    target: Option<String>,
    lanes: bool,
) -> Result<()> {
    if lanes && group.is_none() {
        return Err(anyhow!("--lanes requires --group"));
    }
    let group = resolve_group(group);
    cmd::merge::run(store, task_id.as_deref(), group.as_deref(), approve, check, force, target.as_deref(), lanes)
}

pub(super) fn accept(store: Arc<store::Store>, task_id: String) -> Result<()> {
    let principal = local_principal();
    crate::artifact_custody::accept(&store, &task_id, &principal)?;
    println!("[aid] Principal {principal} accepted task {task_id}; artifacts remain preserved");
    Ok(())
}

pub(super) fn reject(store: Arc<store::Store>, task_id: String) -> Result<()> {
    let principal = local_principal();
    crate::artifact_custody::reject(&store, &task_id, &principal)?;
    println!("[aid] Principal {principal} rejected task {task_id}; artifacts remain preserved");
    Ok(())
}

pub(super) fn gc(store: Arc<store::Store>, task_id: String) -> Result<()> {
    crate::artifact_custody::delete_accepted_worktree(&store, &task_id)?;
    println!("[aid] Deleted durable artifacts for accepted task {task_id}");
    Ok(())
}

fn local_principal() -> String {
    std::env::var("USER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "local-principal".to_string())
}

pub(super) fn respond(task_id: String, input: Option<String>, file: Option<String>) -> Result<()> {
    cmd::respond::run(&task_id, input.as_deref(), file.as_deref())
}

pub(super) fn reply(
    store: Arc<store::Store>,
    task_id: String,
    message: Option<String>,
    file: Option<String>,
    async_mode: bool,
    timeout_secs: u64,
) -> Result<()> {
    match cmd::reply::run(
        &store,
        &task_id,
        message.as_deref(),
        file.as_deref(),
        async_mode,
        timeout_secs,
    )? {
        cmd::reply::ReplyOutcome::Queued { id } => println!("Queued reply {id} for {task_id}"),
        cmd::reply::ReplyOutcome::Acked { delivered } => {
            println!("Reply acknowledged for {task_id} (delivered: {delivered})");
        }
        cmd::reply::ReplyOutcome::TimedOut { delivered } => {
            println!("Reply timed out for {task_id} (delivered: {delivered})");
        }
    }
    Ok(())
}

pub(super) fn stop(
    store: Arc<store::Store>,
    task_id: String,
    force: bool,
    retry_tree: bool,
) -> Result<()> {
    if retry_tree {
        cmd::stop::stop_retry_tree(&store, &task_id, force)
    } else if force {
        cmd::stop::kill(&store, &task_id)
    } else {
        cmd::stop::stop(&store, &task_id)
    }
}

pub(super) fn kill(store: Arc<store::Store>, task_id: String) -> Result<()> {
    cmd::stop::kill(&store, &task_id)
}

pub(super) fn steer(store: Arc<store::Store>, task_id: String, message: String) -> Result<()> {
    cmd::steer::run(&store, &task_id, &message)
}

pub(super) fn unstick(
    store: Arc<store::Store>,
    task_id: String,
    message: Option<String>,
    escalate: bool,
) -> Result<()> {
    cmd::unstick::run(&store, &task_id, message.as_deref(), escalate)
}
