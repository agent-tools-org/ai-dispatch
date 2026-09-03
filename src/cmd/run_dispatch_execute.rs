// Execution helpers for `aid run` after dispatch setup and prompt assembly.
// Exports: load_runtime_hooks(), run_background_task(), run_foreground_task().
// Deps: hooks, background, container/sandbox wrappers, run lifecycle modules.
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use crate::background::{self, BackgroundRunSpec};
use crate::commit;
use crate::hooks;
use crate::store::Store;
use crate::types::TaskId;
use super::run_dispatch_prepare::PreparedDispatch;
use super::{RunArgs, run_foreground_watch, run_prompt};

pub(super) fn load_runtime_hooks(args: &RunArgs) -> Result<Vec<hooks::Hook>> {
    let mut runtime_hooks = hooks::load_hooks()?;
    runtime_hooks.extend(hooks::parse_cli_hooks(&args.hooks)?);
    Ok(runtime_hooks)
}

pub(super) fn maybe_record_start_sha(
    store: &Arc<Store>,
    task_id: &TaskId,
    effective_dir: Option<&String>,
) -> Result<()> {
    if let Some(dir) = effective_dir
        && let Ok(start_sha) = commit::head_sha(dir)
    {
        store.update_start_sha(task_id.as_str(), &start_sha)?;
    }
    Ok(())
}

fn capture_pre_task_dirty_paths(dir: Option<&String>) -> Option<Vec<String>> {
    let dir = dir?;
    match crate::worktree::capture_worktree_snapshot(Path::new(dir)) {
        Ok(snapshot) => Some(snapshot.status_lines),
        Err(err) => {
            aid_warn!("[aid] rescue: failed to capture pre-task dirty baseline in {dir}: {err}");
            None
        }
    }
}

pub(super) fn run_background_task(
    store: &Arc<Store>,
    args: &RunArgs,
    prepared: &PreparedDispatch,
    prompt_bundle: &run_prompt::PromptBundle,
) -> Result<()> {
    background::check_worker_capacity(store)?;
    let pre_task_dirty_paths = if args.read_only || args.audit_report_mode {
        None
    } else {
        capture_pre_task_dirty_paths(prepared.effective_dir.as_ref())
    };
    let spec = BackgroundRunSpec {
        task_id: prepared.task_id.as_str().to_string(),
        worker_pid: None,
        agent_name: prepared.agent_display_name.clone(),
        prompt: prompt_bundle.effective_prompt.clone(),
        dir: prepared.effective_dir.clone(),
        output: args.output.clone(),
        result_file: args.result_file.clone(),
        result_file_required: args.result_file_required,
        model: prepared.effective_model.clone(),
        budget: prepared.budget_active,
        session_id: args.session_id.clone(),
        verify: args.verify.clone(),
        setup: args.setup.clone(),
        iterate: args.iterate,
        eval: args.eval.clone(),
        eval_feedback_template: args.eval_feedback_template.clone(),
        judge: args.judge.clone(),
        judge_retry: args.judge_retry,
        max_duration_mins: Some(args.timeout_policy.max_duration_mins()),
        max_duration_secs: Some(args.timeout_policy.max_duration.as_secs()),
        idle_timeout_secs: Some(args.timeout_policy.idle.as_secs()),
        max_task_cost: args.max_task_cost,
        retry: args.retry,
        group: args.group.clone(),
        skills: args.skills.clone(),
        checklist: args.checklist.clone(),
        hooks: args.hooks.clone(),
        template: args.template.clone(),
        worktree: args.worktree.clone(),
        base_branch: args.base_branch.clone(),
        peer_review: args.peer_review.clone(),
        audit: args.audit,
        audit_explicit: args.audit_explicit,
        no_audit: args.no_audit,
        scope: args.scope.clone(),
        interactive: true,
        on_done: args.on_done.clone(),
        cascade: args.cascade.clone(),
        parent_task_id: args.parent_task_id.clone(),
        env: args.env.clone(),
        env_forward: args.env_forward.clone(),
        agent_pid: None,
        sandbox: args.sandbox,
        read_only: args.read_only,
        audit_report_mode: args.audit_report_mode,
        container: args.container.clone(),
        link_deps: args.link_deps,
        pre_task_dirty_paths,
        foreground: args.foreground,
    };
    background::save_spec(&spec)?;
    #[cfg(test)]
    let worker_pid = std::process::id();
    #[cfg(not(test))]
    let (_launcher, worker_pid) = match background::spawn_worker(prepared.task_id.as_str()) {
        Ok(worker) => worker,
        Err(err) => {
            let _ = background::clear_spec(prepared.task_id.as_str());
            crate::task_lifecycle::mark_failed(store.as_ref(), &prepared.task_id)?;
            run_prompt::notify_task_completion(store, &prepared.task_id)?;
            return Err(err);
        }
    };
    if let Err(err) = background::update_worker_pid(prepared.task_id.as_str(), worker_pid) {
        background::kill_process(worker_pid);
        let _ = background::clear_spec(prepared.task_id.as_str());
        crate::task_lifecycle::mark_failed(store.as_ref(), &prepared.task_id)?;
        run_prompt::notify_task_completion(store, &prepared.task_id)?;
        return Err(err);
    }
    if let Some(wt_path) = prepared.wt_path.as_deref()
        && let Err(holder) = crate::worktree::rekey_worktree_lock_to_worker(
            Path::new(wt_path),
            prepared.task_id.as_str(),
            worker_pid,
        )
    {
        background::kill_process(worker_pid);
        let _ = background::clear_spec(prepared.task_id.as_str());
        crate::task_lifecycle::mark_failed(store.as_ref(), &prepared.task_id)?;
        run_prompt::notify_task_completion(store, &prepared.task_id)?;
        anyhow::bail!("Worktree {wt_path} lock is owned by task {holder}; background dispatch aborted");
    }
    #[cfg(test)]
    {
        let worker_store = Arc::clone(store);
        let worker_task_id = prepared.task_id.as_str().to_string();
        tokio::spawn(async move {
            let _ = background::run_task(worker_store, &worker_task_id).await;
        });
    }
    if args.announce {
        println!("{}", crate::cmd_dispatch::background_status_line(
            &prepared.task_id,
            &prepared.agent_display_name,
            &args.prompt,
        ));
        aid_hint!("[aid] Watch: aid watch --wait {}", prepared.task_id);
    }
    Ok(())
}

pub(super) async fn run_foreground_task(
    store: &Arc<Store>,
    args: &RunArgs,
    prepared: &PreparedDispatch,
    prompt_bundle: &run_prompt::PromptBundle,
) -> Result<Option<TaskId>> {
    if args.announce {
        println!(
            "Task {} started ({}: {})",
            prepared.task_id,
            prepared.agent_display_name,
            crate::agent::truncate::truncate_text(&args.prompt, 50)
        );
    }
    let mut worker_args = args.clone();
    worker_args.announce = false;
    worker_args.foreground = true;
    run_background_task(store, &worker_args, prepared, prompt_bundle)?;
    let final_task_id = run_foreground_watch::wait_for_task(store, &prepared.task_id).await?;
    if final_task_id != prepared.task_id {
        return Ok(Some(final_task_id));
    }
    Ok(None)
}
