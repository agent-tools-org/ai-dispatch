// Handler for `aid retry` plus a silent helper that returns the new task id.
// Reuses the original task config and dispatches a child task with feedback.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

use crate::cmd::run::{self, switch_agent, RunArgs};
use crate::agent::model_validation::ModelSource;
use crate::store::Store;
use crate::types::TaskId;

pub struct RetryArgs {
    pub task_id: String,
    pub feedback: Option<String>,
    pub feedback_file: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub idle_timeout_secs: Option<u64>,
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
    let feedback = resolve_feedback(args.feedback.as_deref(), args.feedback_file.as_deref())?;
    let prompt = format!(
        "[Previous attempt feedback]\n{feedback}\n\n[Original task]\n{prompt}",
        prompt = task.prompt,
    );
    let worktree = reusable_worktree(task);
    let (dir, worktree_arg) = if args.dir.is_some() {
        (args.dir.clone(), None) // --dir override takes precedence
    } else {
        resolve_retry_target(task, worktree, &args.task_id, args.reset)?
    };
    if announce {
        println!("Retrying {} with feedback: {}", task.id, truncate(&feedback, 60));
    }
    let mut run_args = load_retry_run_args(store, task)?;
    apply_retry_route(&mut run_args, task, &args);
    finish_retry_run_args(&mut run_args, task, args, prompt, dir, worktree_arg, announce)?;
    Ok(run_args)
}

fn load_retry_run_args(store: &Store, task: &crate::types::Task) -> Result<RunArgs> {
    let mut run_args = RunArgs::saved_for_task(store, task.id.as_str())?.unwrap_or_else(|| {
        RunArgs {
            repo: task.repo_path.clone(),
            dir: task.repo_path.clone(),
            output: task.output_path.clone(),
            model: task.requested_model.clone(),
            // requested_model stores the effective model, not whether the
            // original invocation supplied --model. Old rows cannot retain
            // that distinction, so let stale preferences degrade on retry.
            model_source: ModelSource::AidResolved,
            group: task.workgroup_id.clone(),
            verify: task.verify.clone(),
            read_only: task.read_only,
            budget: task.budget,
            ..Default::default()
        }
    });
    run_args.repo = run_args.repo.or_else(|| task.repo_path.clone());
    Ok(run_args)
}

// Anchor the current route so switch_agent can compare "task's agent" to
// "next agent" and drop route-owned fields (model + session_id) when they
// differ. A same-agent retry keeps both; a different-agent retry drops both.
// Explicit --model / --idle-timeout then override; unspecified inherits.
fn apply_retry_route(run_args: &mut RunArgs, task: &crate::types::Task, args: &RetryArgs) {
    let agent_name = args
        .agent
        .clone()
        .unwrap_or_else(|| task.agent_display_name().to_string());
    run_args.agent_name = task.agent_display_name().to_string();
    run_args.session_id = if task.agent.supports_session_resume() {
        task.agent_session_id.clone()
    } else {
        None
    };
    switch_agent(run_args, agent_name);
    if let Some(model) = args.model.clone() {
        run_args.model = Some(model);
        run_args.model_source = ModelSource::UserSupplied;
    }
    if let Some(secs) = args.idle_timeout_secs {
        run_args.idle_timeout_secs = Some(secs);
    }
}

fn finish_retry_run_args(
    run_args: &mut RunArgs,
    task: &crate::types::Task,
    args: RetryArgs,
    prompt: String,
    dir: Option<String>,
    worktree_arg: Option<String>,
    announce: bool,
) -> Result<()> {
    run_args.prompt = prompt;
    if let Some(dir) = dir {
        run_args.dir = Some(dir);
    } else if worktree_arg.is_none() {
        resolve_replay_dir(run_args, task)?;
    }
    run_args.worktree = worktree_arg;
    resume_pruned_worktree_at_tip(task, run_args)?;
    run_args.announce = announce;
    run_args.parent_task_id = Some(task.id.as_str().to_string());
    run_args.background = args.bg;
    run_args.existing_task_id = None;
    Ok(())
}

// A task dispatched without --dir (or with `--dir .`) stores `dir: null` (or
// `.`), which means "the process cwd of the invocation" — not a stable replay
// target. On replay, resolve the absolute directory the task actually ran in
// (its effective_dir), falling back to its repo, and refuse to guess when
// neither still exists.
fn resolve_replay_dir(run_args: &mut RunArgs, task: &crate::types::Task) -> Result<()> {
    let dir_is_process_cwd = run_args
        .dir
        .as_deref()
        .map(str::trim)
        .map_or(true, |dir| dir.is_empty() || dir == ".");
    if !dir_is_process_cwd {
        return Ok(());
    }
    if let Some(effective) = task.effective_dir.as_deref()
        && Path::new(effective).is_dir()
    {
        run_args.dir = Some(effective.to_string());
        return Ok(());
    }
    if let Some(repo) = task.repo_path.as_deref()
        && Path::new(repo).is_dir()
    {
        run_args.dir = Some(repo.to_string());
        return Ok(());
    }
    anyhow::bail!(
        "cannot retry task {}: recorded working directory no longer exists and no usable repo path; refusing to guess a working directory",
        task.id
    );
}

fn resolve_feedback(feedback: Option<&str>, feedback_file: Option<&str>) -> Result<String> {
    match (feedback, feedback_file) {
        (Some(_), Some(_)) => {
            anyhow::bail!("Cannot use both --feedback and --feedback-file")
        }
        (Some(text), None) => Ok(text.to_string()),
        (None, Some(path)) => std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read feedback file: {path}")),
        (None, None) => anyhow::bail!("Either --feedback or --feedback-file is required"),
    }
}

fn resume_pruned_worktree_at_tip(task: &crate::types::Task, run_args: &mut RunArgs) -> Result<()> {
    let Some(path) = task.worktree_path.as_deref() else { return Ok(()) };
    if Path::new(path).exists() { return Ok(()) }
    let Some(branch) = run_args.worktree.as_deref() else { return Ok(()) };
    let Some(repo) = run_args.repo.as_deref() else { return Ok(()) };
    if let Some(base) = crate::worktree::branch_tip_resume_base(Path::new(repo), branch)? {
        run_args.base_branch = Some(base);
    }
    Ok(())
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

#[cfg(test)]
#[path = "retry_pruned_resume_tests.rs"]
mod pruned_resume_tests;

#[cfg(test)]
#[path = "retry_override_tests.rs"]
mod override_tests;

#[cfg(test)]
#[path = "retry_dir_fallback_tests.rs"]
mod dir_fallback_tests;
