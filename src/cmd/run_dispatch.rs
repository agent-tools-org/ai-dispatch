// Main dispatch flow for `aid run`.
// Exports: run().
// Deps: run setup/execution helpers, prompt builder, lifecycle wrappers, workspace guard.
use anyhow::Result;
use chrono::Local;
use std::sync::Arc;
use crate::agent;
use crate::hooks;
use crate::cmd::show;
use crate::store::Store;
use crate::store::TaskCompletionUpdate;
use crate::types::{EventKind, TaskEvent, TaskId, TaskStatus};
use super::run_bestof;
use super::run_dispatch_execute::{
    load_runtime_hooks, maybe_record_start_sha, run_background_task,
    run_foreground_task,
};
use super::run_dispatch_prepare::{PreparedDispatch, prepare_dispatch};
use super::run_prompt;
use super::{RunArgs, preview_prompt};

pub async fn run(store: Arc<Store>, mut args: RunArgs) -> Result<TaskId> {
    if let Some(n) = args.best_of {
        return Box::pin(run_bestof::run_best_of(store, args, n)).await;
    }
    if args.repo_root.is_none()
        && !args.suppress_nested_repo_warning
        && args.worktree.is_some()
    {
        crate::repo_root::warn_if_nested_repo(args.repo.as_deref().or(args.dir.as_deref()).unwrap_or("."));
    }
    let prepared = prepare_dispatch(&store, &mut args)?;
    let prompt_bundle = run_prompt::build_prompt_bundle(
        &store,
        &args,
        &prepared.agent_kind,
        prepared.workgroup.as_ref(),
        &prepared.requested_skills,
        prepared.task_id.as_str(),
    )?;
    store.update_resolved_prompt(prepared.task_id.as_str(), &prompt_bundle.effective_prompt)?;
    store.update_prompt_tokens(prepared.task_id.as_str(), prompt_bundle.prompt_tokens)?;
    if args.dry_run {
        return dry_run(&store, &prepared, &args, &prompt_bundle);
    }
    ensure_agent_binary_available(&store, &prepared, &args)?;
    let runtime_hooks = load_runtime_hooks(&args)?;
    maybe_record_start_sha(&store, &prepared.task_id, prepared.effective_dir.as_ref())?;
    if !crate::task_lifecycle::mark_running(store.as_ref(), &prepared.task_id)? {
        anyhow::bail!(
            "Task {} could not transition to running (status changed underneath dispatch — likely stopped or timed out); aborting dispatch",
            prepared.task_id
        );
    }
    run_before_hook(
        &store,
        &prepared,
        &runtime_hooks,
    )?;
    if args.background {
        run_background_task(&store, &args, &prepared, &prompt_bundle)?;
    } else if let Some(retry_id) = run_foreground_task(
        &store,
        &args,
        &prepared,
        &prompt_bundle,
    )
    .await?
    {
        return Ok(retry_id);
    }
    Ok(prepared.task_id)
}

fn ensure_agent_binary_available(
    store: &Arc<Store>,
    prepared: &PreparedDispatch,
    args: &RunArgs,
) -> Result<()> {
    if args.container.is_some() || args.sandbox {
        return Ok(());
    }
    if let Err(err) =
        agent::ensure_agent_binary_available(prepared.agent_kind, &prepared.agent_display_name)
    {
        let detail = err.to_string();
        crate::task_lifecycle::complete_task_atomic(
            store.as_ref(),
            TaskCompletionUpdate {
                id: prepared.task_id.as_str(),
                status: TaskStatus::Failed,
                tokens: None,
                duration_ms: 0,
                // Dispatch failed before the agent ran: the request is already on
                // the row, and no model was observed.
                observed_model: None,
                attribution_source: None,
                cost_usd: None,
                exit_code: None,
            },
            &TaskEvent {
                task_id: prepared.task_id.clone(),
                timestamp: Local::now(),
                event_kind: EventKind::Error,
                detail,
                metadata: None,
            },
        )?;
        return Err(err);
    }
    Ok(())
}

fn dry_run(
    store: &Arc<Store>,
    prepared: &PreparedDispatch,
    args: &RunArgs,
    prompt_bundle: &run_prompt::PromptBundle,
) -> Result<TaskId> {
    // A dry run builds a real task row in order to resolve the prompt, then
    // returns without dispatching. Left in `pending`, the background reaper
    // found it ten minutes later and recorded "Task timed out in pending state
    // after 602s (reason: unknown)" — a failure that never happened, against an
    // agent that was never invoked.
    //
    // That is not only board noise: `agent_success_rates` counts
    // `done|merged|failed`, so every dry run quietly lowered an agent's score in
    // the history `aid advise` recommends from. Sixteen such rows accumulated in
    // a single day of verification runs, and agy's 30-day success rate read
    // 73.7% where the truth was 79.7%.
    //
    // `Skipped` is the honest terminal state — the task was deliberately not
    // executed — and it is excluded from both the reaper and the success-rate
    // queries.
    crate::task_lifecycle::mark_skipped(store.as_ref(), prepared.task_id.as_str())?;
    let estimated_cost = crate::cost::estimate_cost(
        prompt_bundle.prompt_tokens,
        prepared.effective_model.as_deref(),
        prepared.agent_kind,
    );
    println!("[dry-run] Task: {}", prepared.task_id);
    println!("[dry-run] Agent: {}", prepared.agent_display_name);
    println!(
        "[dry-run] Prompt: {}",
        preview_prompt(&prompt_bundle.effective_prompt, 200)
    );
    if !prompt_bundle.context_files.is_empty() {
        println!("[dry-run] Context: {}", prompt_bundle.context_files.join(", "));
    }
    if !prepared.requested_skills.is_empty() {
        println!("[dry-run] Skills: {}", prepared.requested_skills.join(", "));
    }
    println!("[dry-run] Estimated tokens: ~{}", prompt_bundle.prompt_tokens);
    println!(
        "[dry-run] Estimated cost: {}",
        crate::cost::format_cost(estimated_cost)
    );
    let _ = args;
    Ok(prepared.task_id.clone())
}

fn run_before_hook(
    store: &Arc<Store>,
    prepared: &PreparedDispatch,
    runtime_hooks: &[hooks::Hook],
) -> Result<()> {
    let mut task = prepared.task.clone();
    task.status = TaskStatus::Running;
    let before_payload = show::task_hook_json(
        &task,
        &prepared.agent_display_name,
        prepared.effective_dir.as_deref(),
    );
    if let Err(err) = hooks::run_hooks_with(
        "before_run",
        &before_payload,
        Some(&prepared.agent_display_name),
        runtime_hooks,
        true,
    ) {
        crate::task_lifecycle::mark_failed(store.as_ref(), &prepared.task_id)?;
        return Err(err);
    }
    Ok(())
}
