// Agent process lifecycle helpers for `aid run`.
// Exports process dispatch and scope diagnostics.
// Depends on run_prompt, watcher, cost estimation, and store/event types.
use anyhow::Result;
use std::path::Path;
use std::process;
use std::sync::Arc;
use tokio::process::Command;
use crate::store::Store;
use crate::types::TaskId;

use super::run_prompt;

pub(crate) async fn run_agent_process_with_cost(
    caller_owned: bool,
    agent: &dyn crate::agent::Agent,
    cmd: Command,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
    workgroup_id: Option<&str>,
    timeout_policy: crate::config::timeout_policy::TimeoutPolicy,
    max_task_cost: Option<f64>,
    start: std::time::Instant,
) -> Result<()> {
    run_prompt::run_agent_process_impl(run_prompt::RunProcessArgs {
        caller_owned,
        agent,
        cmd,
        task_id,
        store,
        log_path,
        output_path,
        model,
        streaming,
        workgroup_id,
        timeout_policy,
        max_task_cost,
        start,
    })
    .await
}

pub(crate) fn check_worktree_escape(repo_dir: Option<&str>) {
    let dir = repo_dir.unwrap_or(".");
    let output = process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output();
    if let Ok(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        let dirty: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
        if !dirty.is_empty() {
            aid_warn!("[aid] ⚠ Worktree escape detected! Agent modified {} file(s) in main repo:", dirty.len());
            for line in dirty.iter().take(10) {
                aid_warn!("  {line}");
            }
            if dirty.len() > 10 {
                aid_warn!("  ... and {} more", dirty.len() - 10);
            }
            aid_hint!("[aid] Run `git checkout .` to discard, or review with `git diff`");
        }
    }
}

/// Check if the agent's diff contains files outside the declared scope.
/// Scope entries can be file paths or directory prefixes (e.g. "src/agent/").
pub(crate) fn check_scope_violations(store: &Store, task_id: &TaskId, scope: &[String], dir: Option<&str>) {
    let work_dir = dir.unwrap_or(".");
    let output = process::Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(work_dir)
        .output();
    let Ok(o) = output else { return };
    if !o.status.success() { return }
    let stdout = String::from_utf8_lossy(&o.stdout);
    let changed: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    if changed.is_empty() { return }
    let violations: Vec<&str> = changed.iter().copied().filter(|file| {
        !scope.iter().any(|s| {
            let s = s.trim_end_matches('/');
            *file == s || file.starts_with(&format!("{s}/"))
        })
    }).collect();
    if violations.is_empty() { return }
    aid_warn!(
        "[aid] Scope violation: {} file(s) modified outside scope",
        violations.len()
    );
    for f in violations.iter().take(10) {
        aid_warn!("  {f}");
    }
    if violations.len() > 10 {
        aid_warn!("  ... and {} more", violations.len() - 10);
    }
    aid_warn!("[aid] Declared scope: {}", scope.join(", "));
    let event = crate::types::TaskEvent::new(
        task_id.clone(),
        crate::types::EventKind::Milestone,
        format!("Scope violation: {} file(s) outside scope", violations.len()),
    );
    let _ = store.insert_event(&event);
}

#[cfg(test)]
#[path = "agent/tests.rs"]
mod tests;
