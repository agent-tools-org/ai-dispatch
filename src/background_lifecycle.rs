// Background lifecycle handoff after a detached worker exits.
// Exports run_post_lifecycle to reuse foreground post-run phases.
// Deps: cmd::run lifecycle API, hooks, Store, and BackgroundRunSpec.

use anyhow::Result;
use std::sync::Arc;

use super::BackgroundRunSpec;
use crate::agent::Agent;
use crate::agent::model_validation::ModelSource;
use crate::store::Store;
use crate::types::{TaskId, TaskStatus};

pub(super) async fn run_post_lifecycle(
    store: &Arc<Store>,
    spec: &BackgroundRunSpec,
    agent: &dyn Agent,
    container_name: Option<&str>,
) -> Result<()> {
    let model_source = crate::cmd::run::RunArgs::saved_for_task(store, &spec.task_id)?
        .map(|args| args.model_source)
        .unwrap_or(ModelSource::AidResolved);
    let lifecycle_args = run_args_from_spec(spec, model_source);
    let task_id = TaskId(spec.task_id.clone());
    let pre_verify_status = store
        .get_task(&spec.task_id)?
        .map(|task| task.status)
        .unwrap_or(TaskStatus::Done);
    let runtime_hooks = load_background_runtime_hooks(&lifecycle_args)?;
    let prompt_bundle = prompt_bundle_from_spec(spec);
    let (repo_path, wt_path) = task_lifecycle_paths(store, &task_id)?;
    crate::cmd::run::post_run_lifecycle(
        if spec.foreground {
            crate::cmd::run::LifecycleMode::Foreground
        } else {
            crate::cmd::run::LifecycleMode::Background
        },
        store,
        &task_id,
        &lifecycle_args,
        agent.kind(),
        &spec.agent_name,
        spec.dir.as_ref(),
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

fn run_args_from_spec(spec: &BackgroundRunSpec, model_source: ModelSource) -> crate::cmd::run::RunArgs {
    crate::cmd::run::RunArgs {
        agent_name: spec.agent_name.clone(),
        prompt: spec.prompt.clone(),
        dir: spec.dir.clone(),
        output: spec.output.clone(),
        result_file: spec.result_file.clone(),
        result_file_required: spec.result_file_required,
        model: spec.model.clone(),
        model_source,
        worktree: spec.worktree.clone(),
        base_branch: spec.base_branch.clone(),
        group: spec.group.clone(),
        verify: spec.verify.clone(),
        setup: spec.setup.clone(),
        iterate: spec.iterate,
        eval: spec.eval.clone(),
        eval_feedback_template: spec.eval_feedback_template.clone(),
        judge: spec.judge.clone(),
        judge_retry: spec.judge_retry,
        peer_review: spec.peer_review.clone(),
        max_duration_mins: spec.max_duration_mins,
        max_task_cost: spec.max_task_cost,
        idle_timeout_secs: spec.idle_timeout_secs,
        retry: spec.retry,
        checklist: spec.checklist.clone(),
        skills: spec.skills.clone(),
        hooks: spec.hooks.clone(),
        template: spec.template.clone(),
        parent_task_id: spec.parent_task_id.clone(),
        cascade: spec.cascade.clone(),
        read_only: spec.read_only,
        audit_report_mode: spec.audit_report_mode,
        sandbox: spec.sandbox,
        container: spec.container.clone(),
        budget: spec.budget,
        session_id: spec.session_id.clone(),
        env: spec.env.clone(),
        env_forward: spec.env_forward.clone(),
        audit: spec.audit,
        audit_explicit: spec.audit_explicit,
        no_audit: spec.no_audit,
        scope: spec.scope.clone(),
        link_deps: spec.link_deps,
        background: true,
        announce: spec.foreground,
        foreground: spec.foreground,
        ..Default::default()
    }
}

fn prompt_bundle_from_spec(spec: &BackgroundRunSpec) -> crate::cmd::run::PromptBundle {
    crate::cmd::run::PromptBundle {
        effective_prompt: spec.prompt.clone(),
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
