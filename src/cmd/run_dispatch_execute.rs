// Execution helpers for `aid run` after dispatch setup and prompt assembly.
// Exports: load_runtime_hooks(), maybe_start_container(), run_background_task(), run_foreground_task().
// Deps: hooks, background, container/sandbox wrappers, run lifecycle modules.
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use crate::agent::{self, RunOpts};
use crate::background::{self, BackgroundRunSpec};
use crate::commit;
use crate::hooks;
use crate::store::Store;
use crate::types::{TaskId, TaskStatus};
use super::run_dispatch_prepare::PreparedDispatch;
use super::{RunArgs, run_foreground_guard, run_lifecycle, run_prompt};

pub(super) fn build_run_opts(
    args: &RunArgs,
    prepared: &PreparedDispatch,
    prompt_bundle: &run_prompt::PromptBundle,
) -> RunOpts {
    RunOpts {
        dir: prepared.effective_dir.clone(),
        output: args.output.clone(),
        result_file: args.result_file.clone(),
        model: prepared.effective_model.clone(),
        budget: prepared.budget_active,
        read_only: args.read_only,
        sandbox: args.sandbox,
        context_files: prompt_bundle.context_files.clone(),
        session_id: args.session_id.clone(),
        env: crate::agent::env_with_agent_log(
            args.env.clone(),
            prepared.task_id.as_str(),
            args.container.is_none() && !args.sandbox,
        ),
        env_forward: args.env_forward.clone(),
    }
}

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

pub(super) fn maybe_start_container(
    args: &RunArgs,
    prepared: &PreparedDispatch,
) -> Result<Option<String>> {
    if let Some(image) = args.container.as_deref() {
        let project_dir = prepared
            .effective_dir
            .as_deref()
            .map(Path::new)
            .unwrap_or_else(|| Path::new("."));
        let project_id = prepared
            .detected_project
            .as_ref()
            .map(|project| project.id.as_str())
            .unwrap_or(prepared.task_id.as_str());
        Ok(Some(crate::container::start_or_reuse(
            image,
            project_dir,
            project_id,
        )?))
    } else {
        Ok(None)
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
        model: prepared.effective_model.clone(),
        verify: args.verify.clone(),
        setup: args.setup.clone(),
        iterate: args.iterate,
        eval: args.eval.clone(),
        eval_feedback_template: args.eval_feedback_template.clone(),
        judge: args.judge.clone(),
        max_duration_mins: Some(args.timeout_policy.max_duration_mins()),
        idle_timeout_secs: Some(args.timeout_policy.idle.as_secs()),
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
    };
    background::save_spec(&spec)?;
    let mut worker = match background::spawn_worker(prepared.task_id.as_str()) {
        Ok(worker) => worker,
        Err(err) => {
            let _ = background::clear_spec(prepared.task_id.as_str());
            crate::task_lifecycle::mark_failed(store.as_ref(), &prepared.task_id)?;
            run_prompt::notify_task_completion(store, &prepared.task_id)?;
            return Err(err);
        }
    };
    if let Err(err) = background::update_worker_pid(prepared.task_id.as_str(), worker.id()) {
        let _ = worker.kill();
        let _ = background::clear_spec(prepared.task_id.as_str());
        crate::task_lifecycle::mark_failed(store.as_ref(), &prepared.task_id)?;
        run_prompt::notify_task_completion(store, &prepared.task_id)?;
        return Err(err);
    }
    if let Some(wt_path) = prepared.wt_path.as_deref()
        && let Err(holder) = crate::worktree::rekey_worktree_lock_to_worker(
            Path::new(wt_path),
            prepared.task_id.as_str(),
            worker.id(),
        )
    {
        let _ = worker.kill();
        let _ = background::clear_spec(prepared.task_id.as_str());
        crate::task_lifecycle::mark_failed(store.as_ref(), &prepared.task_id)?;
        run_prompt::notify_task_completion(store, &prepared.task_id)?;
        anyhow::bail!("Worktree {wt_path} lock is owned by task {holder}; background dispatch aborted");
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
    runtime_hooks: &[hooks::Hook],
    container_name: Option<&str>,
) -> Result<Option<TaskId>> {
    let pre_task_dirty_paths = if args.read_only || args.audit_report_mode {
        None
    } else {
        capture_pre_task_dirty_paths(prepared.effective_dir.as_ref())
    };
    let opts = build_run_opts(args, prepared, prompt_bundle);
    let uses_durable_codex_home = agent::should_use_durable_codex_home(
        prepared.agent_kind,
        args.sandbox,
        container_name.is_some(),
    );
    let cargo_target_dir = agent::rust_build_cache_target_dir(
        prepared.effective_dir.as_deref(),
        args.worktree.as_deref(),
    );
    let codex_resume_fallback = uses_durable_codex_home
        && opts
            .session_id
            .as_deref()
            .is_some_and(agent::codex::resume_fallback_needed);
    let mut std_cmd = prepared
        .agent
        .build_command_with_context(
            &prompt_bundle.effective_prompt,
            &opts,
            agent::CommandContext {
                durable_codex_home: uses_durable_codex_home,
                cargo_target_dir: cargo_target_dir.clone(),
            },
        )
        .map_err(|err| anyhow::anyhow!("Failed to build agent command: {err:#}"))?;
    // TODO: integrate credential_pool rotation here
    let _home_guard = agent::apply_run_env(&mut std_cmd, &opts, Some(prepared.task_id.as_str()))?;
    if uses_durable_codex_home {
        agent::apply_codex_home_env(&mut std_cmd)?;
    }
    if codex_resume_fallback {
        store.insert_event(&agent::codex::resume_fallback_event(&prepared.task_id))?;
    }
    if let Some(ref dir) = prepared.effective_dir {
        agent::set_git_ceiling(&mut std_cmd, dir);
    }
    if let Some(ref group) = args.group {
        std_cmd.env("AID_GROUP", group);
    }
    std_cmd.env("AID_TASK_ID", prepared.task_id.as_str());
    let depth = crate::cmd::run::task_depth(store, prepared.task_id.as_str()).unwrap_or(0);
    std_cmd.env("AID_TASK_DEPTH", depth.to_string());
    agent::apply_cargo_target_env(&mut std_cmd, cargo_target_dir.as_deref());
    let std_cmd = if let Some(container_name) = container_name {
        aid_info!(
            "[aid] Container: running {} in {}",
            prepared.agent_kind.as_str(),
            container_name
        );
        crate::container::exec_in_container(&std_cmd, container_name)
    } else if args.sandbox && crate::sandbox::can_sandbox(prepared.agent_kind) {
        if !crate::sandbox::is_available() {
            anyhow::bail!("--sandbox requires Apple Container CLI. Install: brew install container");
        }
        aid_info!(
            "[aid] Sandbox: running {} in container aid-{}",
            prepared.agent_kind.as_str(),
            prepared.task_id
        );
        crate::sandbox::wrap_command(
            &std_cmd,
            prepared.task_id.as_str(),
            prepared.agent_kind,
            args.read_only,
        )
    } else if args.sandbox {
        aid_warn!(
            "[aid] Warning: {} does not support sandbox, running on host",
            prepared.agent_kind.as_str()
        );
        std_cmd
    } else {
        std_cmd
    };
    if args.announce {
        println!(
            "Task {} started ({}: {})",
            prepared.task_id,
            prepared.agent_display_name,
            crate::agent::truncate::truncate_text(&args.prompt, 50)
        );
    }
    let mut spec_guard = run_foreground_guard::save_foreground_spec(
        args,
        prepared,
        prompt_bundle,
        pre_task_dirty_paths.clone(),
    )?;
    run_foreground_guard::run_agent_with_signal(
        &*prepared.agent,
        &prepared.agent_display_name,
        std_cmd,
        &prepared.task_id,
        store,
        &prepared.log_path,
        args.output.as_deref(),
        prepared.effective_model.as_deref(),
        prepared.task.workgroup_id.as_deref(),
        args.timeout_policy,
        args.max_task_cost,
    )
    .await?;
    spec_guard.clear_now()?;
    let pre_verify_status = store
        .get_task(prepared.task_id.as_str())?
        .map(|task| task.status)
        .unwrap_or(TaskStatus::Done);
    run_lifecycle::post_run_lifecycle(
        run_lifecycle::LifecycleMode::Foreground,
        store,
        &prepared.task_id,
        args,
        prepared.agent_kind,
        &prepared.agent_display_name,
        prepared.effective_dir.as_ref(),
        prepared.repo_path.as_ref(),
        prepared.wt_path.as_ref(),
        container_name,
        runtime_hooks,
        prompt_bundle,
        pre_verify_status,
        pre_task_dirty_paths.as_deref(),
    )
    .await
}
