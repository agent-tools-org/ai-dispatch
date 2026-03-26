// Handler for `aid run <agent> <prompt>` — dispatch a task to an AI CLI.
// Orchestrates RunArgs, prompt construction, verification hooks, and retry logic.
// Depends on agents, hooks, store, and run_agent helpers for process lifecycle work.
use anyhow::{Context, Result};
use chrono::Local;
use crate::agent::{self, RunOpts};
use crate::agent_config;
use crate::background::{self, BackgroundRunSpec};
use crate::cmd::{config as cmd_config, judge, retry_logic, show};
use crate::config;
use crate::hooks;
use crate::paths;
use crate::project;
use crate::rate_limit;
use crate::session;
use crate::store::Store;
use crate::types::*;
use crate::usage;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::process::Command;
#[path = "run_prompt.rs"]
mod run_prompt;
#[path = "run_agent.rs"]
mod run_agent;
#[path = "run_bestof.rs"]
mod run_bestof;
#[path = "run_lifecycle.rs"]
mod run_lifecycle;
use self::run_agent::run_agent_process_with_timeout;
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
    pub judge: Option<String>,
    pub peer_review: Option<String>,
    pub max_duration_mins: Option<i64>,
    pub max_task_cost: Option<f64>,
    pub retry: u32,
    pub context: Vec<String>,
    pub checklist: Vec<String>,
    pub skills: Vec<String>,
    pub hooks: Vec<String>,
    pub template: Option<String>,
    pub background: bool,
    pub dry_run: bool,
    pub announce: bool,
    pub parent_task_id: Option<String>,
    pub on_done: Option<String>,
    pub cascade: Vec<String>,
    pub read_only: bool,
    pub sandbox: bool,
    pub container: Option<String>,
    pub budget: bool,
    pub best_of: Option<usize>,
    pub metric: Option<String>,
    pub session_id: Option<String>,
    pub team: Option<String>,
    pub context_from: Vec<String>,
    pub batch_siblings: Vec<(String, String, String)>,
    pub scope: Vec<String>,
    pub env: Option<HashMap<String, String>>,
    pub env_forward: Option<Vec<String>>,
    pub judge_retry: bool,
    pub existing_task_id: Option<TaskId>,
    pub timeout: Option<u64>,
}

fn resolve_max_duration_mins(timeout: Option<u64>, max_duration_mins: Option<i64>) -> Option<i64> {
    max_duration_mins.or_else(|| timeout.map(|secs| secs.div_ceil(60) as i64))
}

fn preview_prompt(prompt: &str, max_chars: usize) -> String {
    let mut preview: String = prompt.chars().take(max_chars).collect();
    if prompt.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

fn context_file_from_spec(spec: &str) -> String {
    spec.split_once(':')
        .map_or_else(|| spec.to_string(), |(file, _)| file.to_string())
}

fn validate_dispatch(args: &RunArgs, agent_kind: &AgentKind) -> Vec<String> {
    let mut warnings = Vec::new();
    let prompt_len = args.prompt.chars().count();
    if prompt_len < 10 {
        warnings.push("Prompt is very short, agent may not have enough context".to_string());
    }
    if matches!(
        agent_kind,
        AgentKind::Codex | AgentKind::OpenCode | AgentKind::Cursor | AgentKind::Kilo | AgentKind::Codebuff
    ) && args.dir.is_none() && !args.read_only
    {
        let profile = agent::classifier::classify(
            &args.prompt,
            agent::classifier::count_file_mentions(&args.prompt),
            prompt_len,
        );
        if !matches!(
            profile.category,
            agent::classifier::TaskCategory::Research | agent::classifier::TaskCategory::Documentation
        ) {
            warnings.push("Code agent without --dir may not be able to write files".to_string());
        }
    }
    if prompt_len > 5000 {
        warnings.push(format!(
            "Very long prompt ({prompt_len} chars), consider using --context files instead"
        ));
    }
    if matches!(agent_kind, AgentKind::Gemini) && args.worktree.is_some() {
        warnings.push("Research agent with --worktree is unusual, did you mean a code agent?".to_string());
    }
    warnings
}
pub async fn run(store: Arc<Store>, mut args: RunArgs) -> Result<TaskId> {
    args.max_duration_mins = resolve_max_duration_mins(args.timeout, args.max_duration_mins);

    if let Some(n) = args.best_of {
        return Box::pin(run_bestof::run_best_of(store, args, n)).await;
    }

    let detected_project = project::detect_project();
    if let Some(project) = detected_project.as_ref() {
        let mut defaults_applied = false;
        if args.max_task_cost.is_none() {
            args.max_task_cost = project.max_task_cost;
        }
        if args.team.is_none()
            && let Some(team) = project.team.as_ref() {
                args.team = Some(team.clone());
                defaults_applied = true;
            }
        if args.verify.is_none()
            && let Some(verify) = project.verify.as_ref() {
                args.verify = Some(verify.clone());
                defaults_applied = true;
            }
        if args.container.is_none()
            && let Some(container) = project.container.as_ref() {
                args.container = Some(container.clone());
                defaults_applied = true;
            }
        if !args.budget && project.budget.prefer_budget {
            args.budget = true;
            defaults_applied = true;
        }
        if defaults_applied {
            aid_info!(
                "[aid] Project '{}' defaults: team={}, verify={}",
                project.id,
                args.team.as_deref().unwrap_or("None"),
                args.verify.as_deref().unwrap_or("None"),
            );
        }
    }

    let (agent_kind, custom_agent_name) = if let Some(kind) = AgentKind::parse_str(&args.agent_name) {
        (kind, None)
    } else if agent::registry::custom_agent_exists(&args.agent_name) {
        (AgentKind::Custom, Some(args.agent_name.clone()))
    } else {
        let custom = agent::registry::list_custom_agents();
        let mut available = "gemini, codex, opencode, cursor, kilo, codebuff".to_string();
        for ca in &custom {
            available.push_str(&format!(", {}", ca.id));
        }
        anyhow::bail!("Unknown agent '{}'. Available: {}", args.agent_name, available);
    };
    // Auto-infer --dir . for code agents when cwd is a git repo
    if args.dir.is_none()
        && args.worktree.is_none()
        && matches!(
            agent_kind,
            AgentKind::Codex | AgentKind::OpenCode | AgentKind::Cursor | AgentKind::Kilo | AgentKind::Codebuff | AgentKind::Droid | AgentKind::Custom
        )
        && std::path::Path::new(".git").exists()
    {
        args.dir = Some(".".to_string());
        aid_info!("[aid] Auto-set --dir . (git repo detected)");
    }
    let agent_display_name = custom_agent_name.as_deref().unwrap_or_else(|| agent_kind.as_str());
    if let Some(info) = rate_limit::get_rate_limit_info(&agent_kind)
        && let Some(ref recovery) = info.recovery_at
    {
        if let Some(next_agent) = args.cascade.first() {
            aid_warn!(
                "[aid] {} is rate-limited — will cascade to {}",
                agent_kind.as_str(),
                next_agent
            );
        } else if let Some(fallback) = crate::agent::selection::coding_fallback_for(&agent_kind) {
            aid_warn!(
                "[aid] {} is rate-limited (until {}), auto-cascading to {}",
                agent_kind.as_str(),
                recovery,
                fallback.as_str()
            );
            args.cascade = vec![fallback.as_str().to_string()];
        } else {
            anyhow::bail!(
                "{} is rate-limited until {}. Use --cascade <agent> to specify a fallback, or wait.",
                agent_kind.as_str(),
                recovery
            );
        }
    }
    let requested_skills = run_prompt::effective_skills(&agent_kind, &args);
    if args.skills.is_empty() {
        for skill in &requested_skills {
            aid_info!("[aid] Auto-applied skill: {skill}");
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
            aid_warn!("[aid] Warning: {}\n[aid] Auto-enabling budget mode", msg);
        }
        true
    } else {
        false
    };
    let requested_model = args.model.clone().or_else(|| agent_config::get_default_model(&args.agent_name));
    let budget_active = args.budget || auto_budget || cfg.selection.budget_mode;
    let effective_model = if budget_active && requested_model.is_none() {
        if let Some(bm) = cmd_config::budget_model(&agent_kind) {
            aid_info!("[aid] Budget mode: using model {}", bm);
            Some(bm.to_string())
        } else {
            requested_model.clone()
        }
    } else {
        requested_model.clone()
    };
    let agent: Box<dyn agent::Agent> = if agent_kind == AgentKind::Custom {
        agent::registry::resolve_custom_agent(custom_agent_name.as_deref().unwrap_or(""))
            .ok_or_else(|| anyhow::anyhow!("Custom agent '{}' not found in registry", args.agent_name))?
    } else {
        agent::get_agent(agent_kind)
    };
    let task_id = match args.existing_task_id.clone() {
        Some(id) => {
            crate::sanitize::validate_task_id(id.as_str())?;
            id
        }
        None => TaskId::generate(),
    };
    let log_path = paths::log_path(task_id.as_str());
    let workgroup = run_prompt::load_workgroup(&store, args.group.as_deref())?;
    let explicit_repo_path = args.repo.as_deref().map(run_prompt::resolve_repo_path).transpose()?;
    // Create worktree if requested, override dir to point into it
    let (wt_path, wt_branch, effective_dir, resolved_repo) = run_prompt::resolve_worktree_paths(&args, explicit_repo_path.as_deref())?;
    // Use resolved repo_path (always set when worktree is created, even without --repo)
    let repo_path = resolved_repo.clone().or(explicit_repo_path);
    // Check worktree lock — prevent concurrent agent access to the same worktree
    if let Some(ref wt) = wt_path {
        if let Some(holder) = crate::worktree::check_worktree_lock(Path::new(wt)) {
            anyhow::bail!(
                "Worktree {wt} is locked by task {holder} — concurrent access prevented. \
                 Use separate worktree names for parallel tasks."
            );
        }
        crate::worktree::write_worktree_lock(Path::new(wt), task_id.as_str());
    }
    if let (Some(wt), Some(repo)) = (wt_path.as_deref(), repo_path.as_deref()) {
        let context_files: Vec<String> =
            args.context.iter().map(|spec| context_file_from_spec(spec)).collect();
        let synced = crate::worktree::sync_context_files_into_worktree(
            Path::new(repo),
            Path::new(wt),
            &context_files,
        );
        if !synced.is_empty() {
            aid_info!(
                "[aid] Synced {} context file(s) into worktree: {}",
                synced.len(),
                synced.join(", ")
            );
        }
    }
    let caller = session::current_caller();
    let mut task = Task {
        id: task_id.clone(),
        agent: agent_kind,
        custom_agent_name: custom_agent_name.clone(),
        prompt: args.prompt.clone(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Pending,
        parent_task_id: args.parent_task_id.clone(),
        workgroup_id: args.group.clone(),
        caller_kind: caller.as_ref().map(|item| item.kind.clone()),
        caller_session_id: caller.as_ref().map(|item| item.session_id.clone()),
        agent_session_id: None,
        repo_path: repo_path.clone(),
        worktree_path: wt_path.clone(),
        worktree_branch: wt_branch,
        log_path: Some(log_path.to_string_lossy().to_string()),
        output_path: args.output.clone(),
        tokens: None,
        prompt_tokens: None,
            duration_ms: None,
            model: effective_model.clone(),
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
        completed_at: None,
        verify: args.verify.clone(),
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: args.read_only,
        budget: args.budget,
    };
    let normalized_prompt = task.prompt.trim().to_lowercase();
    let profile = agent::classifier::classify(
        &task.prompt,
        agent::classifier::count_file_mentions(&normalized_prompt),
        task.prompt.chars().count(),
    );
    task.category = Some(profile.category.label().to_string());
    let dispatch_warnings = validate_dispatch(&args, &agent_kind);
    for warning in &dispatch_warnings {
        aid_warn!("[aid] Warning: {warning}");
    }
    if args.existing_task_id.is_some() && store.get_task(task_id.as_str())?.is_some() {
        store.replace_waiting_task(&task)?;
    } else {
        store.insert_task(&task)?;
    }
    let before_worktree = task.worktree_path.clone();
    let prompt_bundle = run_prompt::build_prompt_bundle(&store, &args, &agent_kind, workgroup.as_ref(), &requested_skills, task_id.as_str())?;
    store.update_resolved_prompt(task_id.as_str(), &prompt_bundle.effective_prompt)?;
    store.update_prompt_tokens(task_id.as_str(), prompt_bundle.prompt_tokens)?;
    if args.dry_run {
        let estimated_cost = crate::cost::estimate_cost(
            prompt_bundle.prompt_tokens,
            effective_model.as_deref(),
            agent_kind,
        );
        println!("[dry-run] Task: {task_id}");
        println!("[dry-run] Agent: {agent_display_name}");
        println!("[dry-run] Prompt: {}", preview_prompt(&prompt_bundle.effective_prompt, 200));
        if !prompt_bundle.context_files.is_empty() {
            println!("[dry-run] Context: {}", prompt_bundle.context_files.join(", "));
        }
        if !requested_skills.is_empty() {
            println!("[dry-run] Skills: {}", requested_skills.join(", "));
        }
        println!("[dry-run] Estimated tokens: ~{}", prompt_bundle.prompt_tokens);
        println!("[dry-run] Estimated cost: {}", crate::cost::format_cost(estimated_cost));
        return Ok(task_id);
    }
    let opts = RunOpts {
        dir: effective_dir.clone(),
        output: args.output.clone(),
        model: effective_model.clone(),
        budget: budget_active,
        read_only: args.read_only,
        context_files: prompt_bundle.context_files.clone(),
        session_id: args.session_id.clone(),
        env: args.env.clone(),
        env_forward: args.env_forward.clone(),
    };
    let mut runtime_hooks = hooks::load_hooks()?;
    runtime_hooks.extend(hooks::parse_cli_hooks(&args.hooks)?);
    let container_name = if let Some(image) = args.container.as_deref() {
        let project_dir = effective_dir.as_deref().map(Path::new).unwrap_or_else(|| Path::new("."));
        let project_id = detected_project.as_ref().map(|project| project.id.as_str()).unwrap_or(task_id.as_str());
        Some(crate::container::start_or_reuse(image, project_dir, project_id)?)
    } else {
        None
    };
    store.update_task_status(task_id.as_str(), TaskStatus::Running)?;
    let before_payload = show::task_hook_json(
        &task_id,
        agent_display_name,
        TaskStatus::Running,
        &args.prompt,
        before_worktree.as_deref(),
        effective_dir.as_deref(),
        None,
    );
    if let Err(err) = hooks::run_hooks_with(
        "before_run",
        &before_payload,
        Some(agent_display_name),
        &runtime_hooks,
        true,
    ) {
        store.update_task_status(task_id.as_str(), TaskStatus::Failed)?;
        return Err(err);
    }
    if args.background {
        background::check_worker_capacity(&store)?;
        let spec = BackgroundRunSpec {
            task_id: task_id.as_str().to_string(),
            worker_pid: None,
            agent_name: agent_display_name.to_string(),
            prompt: prompt_bundle.effective_prompt,
            dir: effective_dir,
            output: args.output.clone(),
            model: effective_model.clone(),
            verify: args.verify.clone(),
            judge: args.judge.clone(),
            max_duration_mins: args.max_duration_mins,
            idle_timeout_secs: crate::idle_timeout::idle_timeout_secs_from_env(args.env.as_ref()),
            retry: args.retry,
            group: args.group.clone(),
            skills: args.skills.clone(),
            checklist: args.checklist.clone(),
            template: args.template.clone(),
            interactive: true,
            on_done: args.on_done.clone(),
            cascade: args.cascade.clone(),
            parent_task_id: args.parent_task_id.clone(),
            env: args.env.clone(),
            env_forward: args.env_forward.clone(),
            agent_pid: None,
            sandbox: args.sandbox,
            read_only: args.read_only,
            container: args.container.clone(),
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
                agent_display_name,
                crate::agent::truncate::truncate_text(&args.prompt, 50)
            );
            aid_hint!("[aid] Watch: aid watch --quiet {task_id}");
        }
    } else {
        let mut std_cmd = agent
            .build_command(&prompt_bundle.effective_prompt, &opts)
            .context("Failed to build agent command")?;
        agent::apply_run_env(&mut std_cmd, &opts);
        if let Some(ref dir) = effective_dir {
            agent::set_git_ceiling(&mut std_cmd, dir);
        }
        if let Some(ref group) = args.group {
            std_cmd.env("AID_GROUP", group);
        }
        std_cmd.env("AID_TASK_ID", task_id.as_str());
        if agent::is_rust_project(effective_dir.as_deref())
            && let Some(target_dir) = agent::target_dir_for_worktree(args.worktree.as_deref())
        {
            std_cmd.env("CARGO_TARGET_DIR", &target_dir);
        }
        let std_cmd = if let Some(container_name) = container_name.as_deref() {
            aid_info!(
                "[aid] Container: running {} in {}",
                agent_kind.as_str(),
                container_name
            );
            crate::container::exec_in_container(&std_cmd, container_name)
        } else if args.sandbox && crate::sandbox::can_sandbox(agent_kind) {
            if !crate::sandbox::is_available() {
                anyhow::bail!("--sandbox requires Apple Container CLI. Install: brew install container");
            }
            aid_info!("[aid] Sandbox: running {} in container aid-{}", agent_kind.as_str(), task_id);
            crate::sandbox::wrap_command(&std_cmd, task_id.as_str(), agent_kind)
        } else if args.sandbox {
            aid_warn!("[aid] Warning: {} does not support sandbox, running on host", agent_kind.as_str());
            std_cmd
        } else {
            std_cmd
        };
        if args.announce {
            println!(
                "Task {} started ({}: {})",
                task_id,
                agent_display_name,
                crate::agent::truncate::truncate_text(&args.prompt, 50)
            );
        }
        if agent.needs_pty() {
            // Route through PTY runner for agents that need a terminal
            crate::pty_runner::run_agent_process(
                &*agent,
                &std_cmd,
                &task_id,
                &store,
                &log_path,
                args.output.as_deref(),
                effective_model.as_deref(),
                agent.streaming(),
            )?;
        } else {
            let mut tokio_cmd = Command::from(std_cmd);
            tokio_cmd.stdout(std::process::Stdio::piped());
            tokio_cmd.stderr(std::process::Stdio::piped());
            let is_streaming = agent.streaming();
            run_agent_process_with_timeout(
                &*agent,
                tokio_cmd,
                &task_id,
                &store,
                &log_path,
                args.output.as_deref(),
                effective_model.as_deref(),
                is_streaming,
                task.workgroup_id.as_deref(),
                args.max_duration_mins,
                args.max_task_cost,
            )
            .await?;
        }
        let pre_verify_status = store.get_task(task_id.as_str())?.map(|task| task.status).unwrap_or(TaskStatus::Done);
        if let Some(retry_id) = run_lifecycle::post_run_lifecycle(&store, &task_id, &args, agent_kind, agent_display_name, effective_dir.as_ref(), repo_path.as_ref(), wt_path.as_ref(), container_name.as_deref(), &runtime_hooks, &prompt_bundle, pre_verify_status).await? {
            return Ok(retry_id);
        }
    }
    Ok(task_id)
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "checklist_tests.rs"]
mod checklist_tests;


pub(crate) fn inherit_retry_base_branch(repo_dir: Option<&str>, task: &Task, retry_args: &mut RunArgs) { run_prompt::inherit_retry_base_branch_impl(repo_dir, task, retry_args); }
pub(crate) use run_agent::run_agent_process;
#[cfg(test)]
fn take_next_cascade_agent(args: &RunArgs) -> Option<(String, Vec<String>)> { run_lifecycle::take_next_cascade_agent(args) }
#[cfg(test)]
fn auto_save_task_output(store: &Store, task: &Task) -> Result<()> { run_lifecycle::auto_save_task_output(store, task) }
pub(crate) fn rescue_quota_failed_task(store: &Store, task_id: &TaskId, quota_error_message: Option<&str>) { run_lifecycle::rescue_quota_failed_task(store, task_id, quota_error_message); }
pub(crate) fn read_quota_error_message(task_id: &TaskId) -> Option<String> { run_lifecycle::read_quota_error_message(task_id) }
#[cfg(test)]
fn worktree_is_empty_diff(worktree_dir: &Path) -> Option<bool> { run_lifecycle::worktree_is_empty_diff(worktree_dir) }

pub(crate) fn maybe_cleanup_fast_fail(store: &Store, task_id: &TaskId, task: &Task) { run_prompt::maybe_cleanup_fast_fail_impl(store, task_id, task); }
/// Run verification if --verify was set and a working dir exists.
pub(crate) fn maybe_verify(
    store: &Store,
    task_id: &TaskId,
    verify: Option<&str>,
    dir: Option<&str>,
    container_name: Option<&str>,
) {
    run_prompt::maybe_verify_impl(store, task_id, verify, dir, container_name);
}
pub(crate) async fn maybe_auto_retry_after_verify_failure(store: &Arc<Store>, task_id: &TaskId, args: &RunArgs, pre_verify_status: TaskStatus) -> Result<Option<TaskId>> {
    run_prompt::maybe_auto_retry_after_verify_failure_impl(store, task_id, args, pre_verify_status).await
}
pub(crate) async fn maybe_auto_retry_after_checklist_miss(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    checklist_result: Option<&crate::cmd::checklist_scan::ChecklistResult>,
) -> Result<Option<TaskId>> {
    run_prompt::maybe_auto_retry_after_checklist_miss_impl(store, task_id, args, checklist_result).await
}
pub(crate) async fn maybe_judge_retry(store: &Arc<Store>, args: &RunArgs, task_id: &TaskId) -> Result<Option<TaskId>> {
    if args.judge_retry {
        return Ok(None);
    }
    let judge_agent = match args
        .judge
        .as_deref()
        .map(str::trim)
        .filter(|agent| !agent.is_empty())
    {
        Some(agent) => agent,
        None => return Ok(None),
    };
    let task = match store.get_task(task_id.as_str())? {
        Some(task) => task,
        None => return Ok(None),
    };
    if task.status != TaskStatus::Done {
        return Ok(None);
    }
    let judge_result = judge::judge_task(&task, judge_agent, &args.prompt).await?;
    if judge_result.passed {
        println!("[aid] Judge approved");
        return Ok(None);
    }
    let feedback = judge_result.feedback.trim();
    aid_info!(
        "[aid] Judge requested retry: {}",
        if feedback.is_empty() { "no feedback provided" } else { feedback }
    );
    let mut retry_args = args.clone();
    let root_prompt = retry_logic::root_prompt(store, &task).unwrap_or_else(|| args.prompt.clone());
    retry_args.prompt = format!(
        "[Judge feedback]\n{}\n\n[Original task]\n{root_prompt}",
        if feedback.is_empty() {
            "Judge requested retry without feedback"
        } else {
            feedback
        }
    );
    retry_args.judge_retry = true;
    let retry_id = Box::pin(run(store.clone(), retry_args)).await?;
    Ok(Some(retry_id))
}
