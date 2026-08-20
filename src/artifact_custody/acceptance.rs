// Records explicit principal decisions without mutating task artifacts.
// Exports accept and reject; depends on Store, task state, and Git.

use anyhow::{Context, Result};
use std::path::Path;

use crate::store::{AcceptanceDecision, AcceptanceRecord, Store};

pub(crate) fn accept(store: &Store, task_id: &str, principal_id: &str) -> Result<()> {
    if store
        .latest_acceptance(task_id)?
        .is_some_and(|record| record.decision == AcceptanceDecision::Accepted)
    {
        return Ok(());
    }
    let task = terminal_task(store, task_id)?;
    let worktree = required_path(task.worktree_path.as_deref(), "worktree path")?;
    let head = git_output(worktree, &["rev-parse", "HEAD"])?;
    let branch = git_output(worktree, &["branch", "--show-current"])?;
    let manifest = super::durability::manifest_digest(worktree, &head)?;
    store.record_acceptance(
        task_id,
        &AcceptanceRecord {
            decision: AcceptanceDecision::Accepted,
            principal_id: principal_id.to_string(),
            accepted_head_sha: Some(head),
            accepted_branch: Some(branch),
            manifest_digest: Some(manifest),
        },
        "cli",
    )
}

pub(crate) fn reject(store: &Store, task_id: &str, principal_id: &str) -> Result<()> {
    if store
        .latest_acceptance(task_id)?
        .is_some_and(|record| record.decision == AcceptanceDecision::Rejected)
    {
        return Ok(());
    }
    terminal_task(store, task_id)?;
    store.record_acceptance(
        task_id,
        &AcceptanceRecord {
            decision: AcceptanceDecision::Rejected,
            principal_id: principal_id.to_string(),
            accepted_head_sha: None,
            accepted_branch: None,
            manifest_digest: None,
        },
        "cli",
    )
}

fn terminal_task(store: &Store, task_id: &str) -> Result<crate::types::Task> {
    let task = store
        .get_task(task_id)?
        .with_context(|| format!("Task not found: {task_id}"))?;
    anyhow::ensure!(
        task.status.is_terminal(),
        "Task {task_id} is not complete; principal acceptance is not allowed"
    );
    Ok(task)
}

fn required_path<'a>(value: Option<&'a str>, label: &str) -> Result<&'a Path> {
    value.map(Path::new).with_context(|| format!("Task has no {label}"))
}

pub(super) fn git_output(repo: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
