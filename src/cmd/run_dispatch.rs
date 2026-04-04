// Main dispatch flow for `aid run`.
// Exports: run().
// Deps: run setup/execution helpers, prompt builder, lifecycle wrappers, workspace guard.
use anyhow::Result;
use std::sync::Arc;
use crate::hooks;
use crate::cmd::show;
use crate::store::Store;
use crate::types::{TaskId, TaskStatus};
use super::run_bestof;
use super::run_dispatch_execute::{
    load_runtime_hooks, maybe_record_start_sha, maybe_start_container, run_background_task,
    run_foreground_task,
};
use super::run_dispatch_prepare::{PreparedDispatch, prepare_dispatch};
use super::run_prompt;
use super::{RunArgs, WorkspaceSymlinkGuard, preview_prompt};

pub async fn run(store: Arc<Store>, mut args: RunArgs) -> Result<TaskId> {
    if let Some(n) = args.best_of {
        return Box::pin(run_bestof::run_best_of(store, args, n)).await;
    }
    let prepared = prepare_dispatch(&store, &mut args)?;
    let before_worktree = prepared.task.worktree_path.clone();
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
        return dry_run(&prepared, &args, &prompt_bundle);
    }
    let _workspace_symlink = if args.background {
        None
    } else {
        Some(WorkspaceSymlinkGuard::create(
            prepared.agent_kind,
            args.group.as_deref(),
            prepared.effective_dir.as_deref(),
        )?)
    };
    let runtime_hooks = load_runtime_hooks(&args)?;
    maybe_record_start_sha(&store, &prepared.task_id, prepared.effective_dir.as_ref())?;
    let container_name = maybe_start_container(&args, &prepared)?;
    store.update_task_status(prepared.task_id.as_str(), TaskStatus::Running)?;
    run_before_hook(
        &store,
        &prepared,
        &args,
        before_worktree.as_deref(),
        &runtime_hooks,
    )?;
    if args.background {
        run_background_task(&store, &args, &prepared, &prompt_bundle)?;
    } else if let Some(retry_id) = run_foreground_task(
        &store,
        &args,
        &prepared,
        &prompt_bundle,
        &runtime_hooks,
        container_name.as_deref(),
    )
    .await?
    {
        return Ok(retry_id);
    }
    Ok(prepared.task_id)
}

fn dry_run(
    prepared: &PreparedDispatch,
    args: &RunArgs,
    prompt_bundle: &run_prompt::PromptBundle,
) -> Result<TaskId> {
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
    args: &RunArgs,
    before_worktree: Option<&str>,
    runtime_hooks: &[hooks::Hook],
) -> Result<()> {
    let before_payload = show::task_hook_json(
        &prepared.task_id,
        &prepared.agent_display_name,
        TaskStatus::Running,
        &args.prompt,
        before_worktree,
        prepared.effective_dir.as_deref(),
        None,
    );
    if let Err(err) = hooks::run_hooks_with(
        "before_run",
        &before_payload,
        Some(&prepared.agent_display_name),
        runtime_hooks,
        true,
    ) {
        store.update_task_status(prepared.task_id.as_str(), TaskStatus::Failed)?;
        return Err(err);
    }
    Ok(())
}
