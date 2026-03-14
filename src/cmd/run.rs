// Handler for `aid run <agent> <prompt>` — dispatch a task to an AI CLI.
// Creates task record, spawns agent process, wires watcher, records completion.
use anyhow::{Context, Result};
use chrono::Local;
use std::sync::Arc;
use tokio::process::Command;
use crate::agent::{self, RunOpts};
use crate::background::{self, BackgroundRunSpec};
use crate::cmd::{config as cmd_config, retry_logic};
use crate::config;
use crate::paths;
use crate::rate_limit;
use crate::session;
use crate::store::Store;
use crate::types::*;
use crate::usage;
#[path = "run_prompt.rs"]
mod run_prompt;
pub const NO_SKILL_SENTINEL: &str = "__aid_no_skill__";
#[derive(Clone, Default)]
pub struct RunArgs {
    pub agent_name: String,
    pub prompt: String,
    pub repo: Option<String>,
    pub dir: Option<String>,
    pub output: Option<String>,
    pub model: Option<String>,
    pub worktree: Option<String>,
    pub base_branch: Option<String>,
    pub group: Option<String>,
    pub verify: Option<String>,
    pub max_duration_mins: Option<i64>,
    pub retry: u32,
    pub context: Vec<String>,
    pub skills: Vec<String>,
    pub template: Option<String>,
    pub background: bool,
    pub announce: bool,
    pub parent_task_id: Option<String>,
    pub on_done: Option<String>,
    pub fallback: Option<String>,
    pub read_only: bool,
    pub budget: bool,
    pub session_id: Option<String>,
}
pub async fn run(store: Arc<Store>, args: RunArgs) -> Result<TaskId> {
    let agent_kind = AgentKind::parse_str(&args.agent_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown agent '{}'. Available: gemini, codex, opencode, cursor, kilo",
            args.agent_name
        )
    })?;
    if let Some(info) = rate_limit::get_rate_limit_info(&agent_kind) {
        if let Some(ref recovery) = info.recovery_at {
            eprintln!(
                "[aid] Warning: {} is rate-limited (try again at {})",
                agent_kind.as_str(),
                recovery
            );
            if let Some(ref fallback_name) = args.fallback {
                eprintln!("[aid] Switching to fallback agent: {}", fallback_name);
            } else {
                eprintln!("[aid] Tip: use --fallback <agent> or --agent with `aid retry`");
            }
        }
    }
    let requested_skills = run_prompt::effective_skills(&agent_kind, &args);
    if args.skills.is_empty() {
        for skill in &requested_skills {
            eprintln!("[aid] Auto-applied skill: {skill}");
        }
    }
    let cfg = config::load_config()?;
    let budget_status = usage::check_budget_status(&store, &cfg)?;
    if budget_status.over_limit {
        if let Some(msg) = budget_status.message {
            anyhow::bail!("Budget limit exceeded:\n{msg}");
        } else {
            anyhow::bail!("Budget limit exceeded");
        }
    }
    let auto_budget = if budget_status.near_limit && !cfg.selection.budget_mode {
        if let Some(ref msg) = budget_status.message {
            eprintln!("[aid] Warning: {}\n[aid] Auto-enabling budget mode", msg);
        }
        true
    } else {
        false
    };
    let budget_active = args.budget || auto_budget || cfg.selection.budget_mode;
    let effective_model = if budget_active && args.model.is_none() {
        if let Some(bm) = cmd_config::budget_model(&agent_kind) {
            eprintln!("[aid] Budget mode: using model {}", bm);
            Some(bm.to_string())
        } else {
            args.model.clone()
        }
    } else {
        args.model.clone()
    };
    let agent = agent::get_agent(agent_kind);
    let task_id = TaskId::generate();
    let log_path = paths::log_path(task_id.as_str());
    let workgroup = run_prompt::load_workgroup(&store, args.group.as_deref())?;
    let repo_path = args.repo.as_deref().map(run_prompt::resolve_repo_path).transpose()?;
    // Create worktree if requested, override dir to point into it
    let (wt_path, wt_branch, effective_dir) = run_prompt::resolve_worktree_paths(&args, repo_path.as_deref())?;
    let caller = session::current_caller();
    let task = Task {
        id: task_id.clone(),
        agent: agent_kind,
        prompt: args.prompt.clone(),
        resolved_prompt: None,
        status: TaskStatus::Pending,
        parent_task_id: args.parent_task_id.clone(),
        workgroup_id: args.group.clone(),
        caller_kind: caller.as_ref().map(|item| item.kind.clone()),
        caller_session_id: caller.as_ref().map(|item| item.session_id.clone()),
        agent_session_id: None,
        repo_path: repo_path.clone(),
        worktree_path: wt_path,
        worktree_branch: wt_branch,
        log_path: Some(log_path.to_string_lossy().to_string()),
        output_path: args.output.clone(),
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: effective_model.clone(),
        cost_usd: None,
        created_at: Local::now(),
        completed_at: None,
        verify: args.verify.clone(),
        read_only: args.read_only,
        budget: args.budget,
    };
    store.insert_task(&task)?;
    let prompt_bundle = run_prompt::build_prompt_bundle(&store, &args, &agent_kind, workgroup.as_ref(), &requested_skills)?;
    store.update_resolved_prompt(task_id.as_str(), &prompt_bundle.effective_prompt)?;
    store.update_prompt_tokens(task_id.as_str(), prompt_bundle.prompt_tokens)?;
    let opts = RunOpts {
        dir: effective_dir.clone(),
        output: args.output.clone(),
        model: effective_model.clone(),
        budget: budget_active,
        read_only: args.read_only,
        context_files: prompt_bundle.context_files,
        session_id: args.session_id.clone(),
    };
    if args.background {
        store.update_task_status(task_id.as_str(), TaskStatus::Running)?;
        let spec = BackgroundRunSpec {
            task_id: task_id.as_str().to_string(),
            worker_pid: None,
            agent_name: agent_kind.as_str().to_string(),
            prompt: prompt_bundle.effective_prompt,
            dir: effective_dir,
            output: args.output.clone(),
            model: effective_model.clone(),
            verify: args.verify.clone(),
            max_duration_mins: args.max_duration_mins,
            retry: args.retry,
            group: args.group.clone(),
            skills: args.skills.clone(),
            template: args.template.clone(),
            interactive: true,
            on_done: args.on_done.clone(),
            parent_task_id: args.parent_task_id.clone(),
        };
        background::save_spec(&spec)?;
        let mut worker = match background::spawn_worker(task_id.as_str()) {
            Ok(worker) => worker,
            Err(err) => {
                let _ = background::clear_spec(task_id.as_str());
                store.update_task_status(task_id.as_str(), TaskStatus::Failed)?;
                run_prompt::notify_task_completion(&store, &task_id)?;
                return Err(err);
            }
        };
        if let Err(err) = background::update_worker_pid(task_id.as_str(), worker.id()) {
            let _ = worker.kill();
            let _ = background::clear_spec(task_id.as_str());
            store.update_task_status(task_id.as_str(), TaskStatus::Failed)?;
            run_prompt::notify_task_completion(&store, &task_id)?;
            return Err(err);
        }
        if args.announce {
            println!(
                "Task {} started in background ({}: {})",
                task_id,
                agent_kind,
                crate::agent::truncate::truncate_text(&args.prompt, 50)
            );
            eprintln!("[aid] Watch: aid watch --quiet {task_id}");
        }
    } else {
        let std_cmd = agent
            .build_command(&prompt_bundle.effective_prompt, &opts)
            .context("Failed to build agent command")?;
        let mut tokio_cmd = Command::from(std_cmd);
        if agent::is_rust_project(effective_dir.as_deref())
            && let Some(target_dir) = agent::shared_target_dir()
        {
            tokio_cmd.env("CARGO_TARGET_DIR", &target_dir);
        }
        tokio_cmd.stdout(std::process::Stdio::piped());
        tokio_cmd.stderr(std::process::Stdio::piped());
        store.update_task_status(task_id.as_str(), TaskStatus::Running)?;
        if args.announce {
            println!(
                "Task {} started ({}: {})",
                task_id,
                agent_kind,
                crate::agent::truncate::truncate_text(&args.prompt, 50)
            );
        }
        let is_streaming = agent.streaming();
        run_agent_process(
            &*agent,
            tokio_cmd,
            &task_id,
            &store,
            &log_path,
            args.output.as_deref(),
            effective_model.as_deref(),
            is_streaming,
        )
        .await?;
        let pre_verify_status =
            store.get_task(task_id.as_str())?.map(|task| task.status).unwrap_or(TaskStatus::Done);
        maybe_verify(
            &store,
            &task_id,
            args.verify.as_deref(),
            effective_dir.as_deref(),
        );
        if let Some(task) = store.get_task(task_id.as_str())? {
            maybe_cleanup_fast_fail(&store, &task_id, &task);
        }
        run_prompt::notify_task_completion(&store, &task_id)?;
        crate::webhook::fire_task_webhooks(&store, task_id.as_str()).await;
        if args.announce {
            let status_hint = if let Some(task) = store.get_task(task_id.as_str())? {
                match task.status {
                    TaskStatus::Done => {
                        format!("[aid] Next: aid show {task_id} --diff | aid merge {task_id}")
                    }
                    TaskStatus::Failed => format!(
                        "[aid] Next: aid show {task_id} | aid retry {task_id} -f \"feedback\""
                    ),
                    _ => String::new(),
                }
            } else {
                String::new()
            };
            if !status_hint.is_empty() {
                eprintln!("{status_hint}");
            }
        }
        if let Some(retry_id) =
            maybe_auto_retry_after_verify_failure(&store, &task_id, &args, pre_verify_status)
                .await?
        {
            return Ok(retry_id);
        }
        if let Some(mut retry_args) = retry_logic::prepare_retry(store.clone(), &task_id, &args).await?
        {
            if let Some(task) = store.get_task(task_id.as_str())? {
                inherit_retry_base_branch(args.dir.as_deref(), &task, &mut retry_args);
            }
            Box::pin(run(store, retry_args)).await?;
        } else if let Some(ref fallback_agent) = args.fallback {
            if let Some(task) = store.get_task(task_id.as_str())?
                && task.status == TaskStatus::Failed
            {
                eprintln!(
                    "[aid] Primary agent {} failed, falling back to {fallback_agent}",
                    args.agent_name
                );
                let mut fallback_args = args.clone();
                fallback_args.agent_name = fallback_agent.clone();
                fallback_args.fallback = None;
                fallback_args.parent_task_id = Some(task_id.as_str().to_string());
                Box::pin(run(store, fallback_args)).await?;
            }
        }
    }
    Ok(task_id)
}
pub(crate) fn inherit_retry_base_branch(repo_dir: Option<&str>, task: &Task, retry_args: &mut RunArgs) { run_prompt::inherit_retry_base_branch_impl(repo_dir, task, retry_args); }
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_agent_process(
    agent: &dyn crate::agent::Agent,
    cmd: Command,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
) -> Result<()> { run_prompt::run_agent_process_impl(agent, cmd, task_id, store, log_path, output_path, model, streaming).await }
pub(crate) fn maybe_cleanup_fast_fail(store: &Store, task_id: &TaskId, task: &Task) { run_prompt::maybe_cleanup_fast_fail_impl(store, task_id, task); }
/// Run verification if --verify was set and a working dir exists.
pub(crate) fn maybe_verify(store: &Store, task_id: &TaskId, verify: Option<&str>, dir: Option<&str>) { run_prompt::maybe_verify_impl(store, task_id, verify, dir); }
pub(crate) async fn maybe_auto_retry_after_verify_failure(store: &Arc<Store>, task_id: &TaskId, args: &RunArgs, pre_verify_status: TaskStatus) -> Result<Option<TaskId>> {
    run_prompt::maybe_auto_retry_after_verify_failure_impl(store, task_id, args, pre_verify_status).await
}
