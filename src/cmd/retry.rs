// Handler for `aid retry` plus a silent helper that returns the new task id.
// Reuses the original task config and dispatches a child task with feedback.

use anyhow::{Context, Result};
use std::sync::Arc;

use crate::cmd::run::{self, switch_agent, RunArgs};
use crate::store::Store;
use crate::types::TaskId;

pub struct RetryArgs {
    pub task_id: String,
    pub feedback: String,
    pub agent: Option<String>,
    pub dir: Option<String>,
    pub reset: bool,
    pub bg: bool,
}

pub async fn run(store: Arc<Store>, args: RetryArgs) -> Result<TaskId> {
    let retry_id = retry_task(store, args, true).await?;
    aid_hint!("[aid] Watch: aid watch --wait {}", retry_id);
    aid_hint!("[aid] TUI:   aid watch --tui");
    Ok(retry_id)
}

pub async fn retry_task(store: Arc<Store>, args: RetryArgs, announce: bool) -> Result<TaskId> {
    let task = store
        .get_task(&args.task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", args.task_id))?;
    supersede_live_holder(&store, &task)?;
    let run_args = retry_task_to_run_args(store.as_ref(), &task, args, announce)?;
    run::run(store, run_args).await
}

// `aid retry <id>` supersedes the task's own run: the caller's intent is
// unambiguous, so if the task still holds a live lease on its recorded
// worktree (a stalled run whose worker outlived its status transition), stop
// the worker first and refuse only if it cannot actually be stopped — never
// proceed into a worktree that still has a live process.
fn supersede_live_holder(store: &Arc<Store>, task: &crate::types::Task) -> Result<()> {
    let Some(path) = task.worktree_path.as_deref().map(std::path::Path::new) else {
        return Ok(());
    };
    let Some(holder) = crate::worktree::live_lock_holder_with_store(path, store) else {
        return Ok(());
    };
    if holder != task.id.as_str() {
        anyhow::bail!(
            "Worktree {} is locked by task {holder} — concurrent access prevented. Use separate worktree names for parallel tasks.",
            path.display()
        );
    }
    // Holder is this task itself (or an ancestor): stop the live worker first,
    // then verify the lease is actually released before proceeding.
    crate::cmd::stop::terminate_any(store, &holder)?;
    if let Some(still_holder) = crate::worktree::live_lock_holder_with_store(path, store) {
        anyhow::bail!(
            "Worktree {} is still locked by task {still_holder} — the worker could not be stopped. Refusing to share a live worktree.",
            path.display()
        );
    }
    aid_info!("[aid] Stopped prior run of {holder} before retry");
    Ok(())
}

fn retry_task_to_run_args(
    store: &Store,
    task: &crate::types::Task,
    args: RetryArgs,
    announce: bool,
) -> Result<RunArgs> {
    let prompt = format!(
        "[Previous attempt feedback]\n{feedback}\n\n[Original task]\n{prompt}",
        feedback = args.feedback,
        prompt = task.prompt,
    );
    let worktree = reusable_worktree(task);
    let (dir, worktree_arg) = if args.dir.is_some() {
        (args.dir, None) // --dir override takes precedence
    } else {
        resolve_retry_target(task, worktree, &args.task_id, args.reset)?
    };

    if announce {
        println!(
            "Retrying {} with feedback: {}",
            task.id,
            truncate(&args.feedback, 60)
        );
    }

    let agent_name = args.agent.unwrap_or_else(|| task.agent_display_name().to_string());
    let mut run_args = RunArgs::saved_for_task(store, task.id.as_str())?.unwrap_or_else(|| {
        RunArgs {
            repo: task.repo_path.clone(),
            dir: task.repo_path.clone(),
            output: task.output_path.clone(),
            model: task.requested_model.clone(),
            group: task.workgroup_id.clone(),
            verify: task.verify.clone(),
            read_only: task.read_only,
            budget: task.budget,
            ..Default::default()
        }
    });
    run_args.repo = run_args.repo.or_else(|| task.repo_path.clone());
    // Anchor the current route so switch_agent can compare "task's agent" to
    // "next agent" and drop route-owned fields (model + session_id) when they
    // differ. A same-agent retry keeps both; a different-agent retry drops both.
    run_args.agent_name = task.agent_display_name().to_string();
    run_args.session_id = if task.agent.supports_session_resume() {
        task.agent_session_id.clone()
    } else {
        None
    };
    switch_agent(&mut run_args, agent_name);
    run_args.prompt = prompt;
    if let Some(dir) = dir {
        run_args.dir = Some(dir);
    }
    run_args.worktree = worktree_arg;
    run_args.announce = announce;
    run_args.parent_task_id = Some(task.id.as_str().to_string());
    run_args.background = args.bg;
    run_args.existing_task_id = None;
    Ok(run_args)
}

fn reusable_worktree(task: &crate::types::Task) -> Option<String> {
    // Always return branch name if the original task used a worktree,
    // even if the worktree was auto-cleaned after failure.
    // The retry will reuse the existing worktree or recreate it.
    if task.worktree_path.is_some() {
        task.worktree_branch.clone()
    } else {
        None
    }
}

fn resolve_retry_target(
    task: &crate::types::Task,
    worktree: Option<String>,
    task_id: &str,
    reset: bool,
) -> Result<(Option<String>, Option<String>)> {
    match task.worktree_path.as_ref() {
        Some(path) if std::path::Path::new(path).exists() => {
            crate::worktree::ensure_consumed_worktree_path_is_isolated(
                task.repo_path.as_deref(),
                path,
                &format!("recorded worktree path for task {}", task.id),
            )?;
            if reset {
                reset_dirty_worktree(path)?;
            } else {
                save_partial_work(path, task_id)?;
            }
            Ok((Some(path.clone()), worktree))
        }
        Some(_) => {
            // Worktree was cleaned up (e.g. auto-cleanup after failure) —
            // pass branch name so run::run recreates a fresh worktree
            Ok((None, worktree))
        }
        None => Ok((None, None)),
    }
}

fn save_partial_work(path: &str, task_id: &str) -> Result<()> {
    if worktree_is_dirty(path)? {
        let mut add_args = vec!["add", "-A", "--", "."];
        add_args.extend_from_slice(crate::worktree::AID_ADD_EXCLUDES);
        run_git(path, &add_args)?;
        run_git(path, &["commit", "-m", &format!("[aid] partial work from {task_id}")])?;
        aid_info!("[aid] Saved partial work from prior attempt as commit");
    }
    Ok(())
}

fn reset_dirty_worktree(path: &str) -> Result<()> {
    if worktree_is_dirty(path)? {
        aid_info!("[aid] Discarding uncommitted changes from prior attempt (--reset requested): git checkout . && git clean -fd");
        run_git(path, &["checkout", "."])?;
        run_git(path, &["clean", "-fd"])?;
    }
    Ok(())
}

fn worktree_is_dirty(path: &str) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["-C", path, "status", "--porcelain"])
        .output()?;
    Ok(output.status.success() && !output.stdout.is_empty())
}

fn run_git(path: &str, args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let safe = s.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &s[..safe])
    }
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "retry_saved_args_tests.rs"]
mod saved_args_tests;
