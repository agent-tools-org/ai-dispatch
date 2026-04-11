// Main dispatch flow for `aid run`.
// Exports: run().
// Deps: run setup/execution helpers, prompt builder, lifecycle wrappers, workspace guard.
use anyhow::Result;
use chrono::Local;
use std::sync::Arc;
use crate::agent::env::which_exists;
use crate::hooks;
use crate::cmd::show;
use crate::store::Store;
use crate::store::TaskCompletionUpdate;
use crate::types::{AgentKind, EventKind, TaskEvent, TaskId, TaskStatus};
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
    if args.repo_root.is_none()
        && !args.suppress_nested_repo_warning
        && args.worktree.is_some()
    {
        crate::repo_root::warn_if_nested_repo(args.repo.as_deref().or(args.dir.as_deref()).unwrap_or("."));
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
    ensure_agent_binary_available(&store, &prepared, &args)?;
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

fn ensure_agent_binary_available(
    store: &Arc<Store>,
    prepared: &PreparedDispatch,
    args: &RunArgs,
) -> Result<()> {
    if args.container.is_some()
        || args.sandbox
        || built_in_agent_binary_exists(prepared.agent_kind, which_exists)
    {
        return Ok(());
    }
    let detail = format!("Agent {} not found. Is it installed?", prepared.agent_display_name);
    store.complete_task_atomic(
        TaskCompletionUpdate {
            id: prepared.task_id.as_str(),
            status: TaskStatus::Failed,
            tokens: None,
            duration_ms: 0,
            model: prepared.effective_model.as_deref(),
            cost_usd: None,
            exit_code: None,
        },
        &TaskEvent {
            task_id: prepared.task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: detail.clone(),
            metadata: None,
        },
    )?;
    Err(anyhow::anyhow!(detail))
}

fn built_in_agent_binary_exists<F>(agent_kind: AgentKind, which: F) -> bool
where
    F: Fn(&str) -> bool,
{
    match agent_kind {
        AgentKind::Codex => which("codex"),
        AgentKind::Copilot => which("copilot"),
        AgentKind::Cursor => which("agent") || which("cursor-agent"),
        AgentKind::Gemini => which("gemini"),
        AgentKind::OpenCode => which("opencode"),
        AgentKind::Kilo => which("kilo"),
        AgentKind::Codebuff => which("aid-codebuff"),
        AgentKind::Droid => which("droid"),
        AgentKind::Oz => which("oz"),
        AgentKind::Claude => which("claude"),
        AgentKind::Custom => true,
    }
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

#[cfg(test)]
mod tests {
    use super::built_in_agent_binary_exists;
    use crate::types::AgentKind;

    #[test]
    fn built_in_agent_binary_exists_rejects_missing_kilo_binary() {
        assert!(!built_in_agent_binary_exists(AgentKind::Kilo, |_| false));
    }

    #[test]
    fn built_in_agent_binary_exists_accepts_cursor_alias_binary() {
        assert!(built_in_agent_binary_exists(AgentKind::Cursor, |name| {
            name == "cursor-agent"
        }));
    }
}
