// Background lifecycle handoff after a detached worker exits.
// Exports run_post_lifecycle to reuse foreground post-run phases.
// Deps: cmd::run lifecycle API, hooks, Store, worker RunArgs and BackgroundRunSpec.

use anyhow::Result;
use std::sync::Arc;

use super::BackgroundRunSpec;
use crate::agent::Agent;
use crate::store::Store;
use crate::types::{TaskId, TaskStatus};

pub(super) async fn run_post_lifecycle(
    store: &Arc<Store>,
    spec: &BackgroundRunSpec,
    lifecycle_args: &crate::cmd::run::RunArgs,
    agent: &dyn Agent,
    container_name: Option<&str>,
) -> Result<()> {
    let task_id = TaskId(spec.task_id.clone());
    let pre_verify_status = store
        .get_task(&spec.task_id)?
        .map(|task| task.status)
        .unwrap_or(TaskStatus::Done);
    let runtime_hooks = load_background_runtime_hooks(lifecycle_args)?;
    let prompt_bundle = prompt_bundle(lifecycle_args);
    let (repo_path, wt_path) = task_lifecycle_paths(store, &task_id)?;
    crate::cmd::run::post_run_lifecycle(
        if spec.foreground {
            crate::cmd::run::LifecycleMode::Foreground
        } else {
            crate::cmd::run::LifecycleMode::Background
        },
        store,
        &task_id,
        lifecycle_args,
        agent.kind(),
        &lifecycle_args.agent_name,
        lifecycle_args.dir.as_ref(),
        repo_path.as_ref(),
        wt_path.as_ref(),
        container_name,
        &runtime_hooks,
        &prompt_bundle,
        pre_verify_status,
        spec.pre_task_dirty_paths.as_deref(),
    )
    .await?;
    Ok(())
}

fn prompt_bundle(args: &crate::cmd::run::RunArgs) -> crate::cmd::run::PromptBundle {
    crate::cmd::run::PromptBundle {
        effective_prompt: args.prompt.clone(),
        context_files: Vec::new(),
        prompt_tokens: 0,
        injected_memory_ids: Vec::new(),
    }
}

fn load_background_runtime_hooks(args: &crate::cmd::run::RunArgs) -> Result<Vec<crate::hooks::Hook>> {
    let mut runtime_hooks = crate::hooks::load_hooks()?;
    runtime_hooks.extend(crate::hooks::parse_cli_hooks(&args.hooks)?);
    Ok(runtime_hooks)
}

fn task_lifecycle_paths(
    store: &Store,
    task_id: &TaskId,
) -> Result<(Option<String>, Option<String>)> {
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok((None, None));
    };
    Ok((task.repo_path, task.worktree_path))
}
