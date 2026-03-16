// Handler for `aid run <agent> <prompt>` — dispatch a task to an AI CLI.
// Orchestrates RunArgs, prompt construction, verification hooks, and retry logic.
// Depends on agents, hooks, store, and run_agent helpers for process lifecycle work.
use anyhow::{Context, Result};
use chrono::Local;
use std::path::Path;
use std::sync::Arc;
use serde_json;
use tokio::process::Command;
use crate::agent::{self, RunOpts};
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
#[path = "run_prompt.rs"]
mod run_prompt;
#[path = "run_agent.rs"]
mod run_agent;
#[path = "run_bestof.rs"]
mod run_bestof;
use self::run_agent::{check_worktree_escape, check_scope_violations, run_agent_process_with_timeout};
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
    pub retry: u32,
    pub context: Vec<String>,
    pub skills: Vec<String>,
    pub hooks: Vec<String>,
    pub template: Option<String>,
    pub background: bool,
    pub announce: bool,
    pub parent_task_id: Option<String>,
    pub on_done: Option<String>,
    pub cascade: Vec<String>,
    pub read_only: bool,
    pub budget: bool,
    pub best_of: Option<usize>,
    pub metric: Option<String>,
    pub session_id: Option<String>,
    pub team: Option<String>,
    pub context_from: Vec<String>,
    pub scope: Vec<String>,
    pub judge_retry: bool,
    pub existing_task_id: Option<TaskId>,
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
    ) && args.dir.is_none()
    {
        warnings.push("Code agent without --dir may not be able to write files".to_string());
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
    if let Some(n) = args.best_of {
        return Box::pin(run_bestof::run_best_of(store, args, n)).await;
    }

    if let Some(project) = project::detect_project() {
        let mut defaults_applied = false;
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
        if !args.budget && project.budget.prefer_budget {
            args.budget = true;
            defaults_applied = true;
        }
        if defaults_applied {
            eprintln!(
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
    let agent_display_name = custom_agent_name
        .as_deref()
        .unwrap_or_else(|| agent_kind.as_str());
    if let Some(info) = rate_limit::get_rate_limit_info(&agent_kind)
        && let Some(ref recovery) = info.recovery_at
    {
        eprintln!(
            "[aid] Warning: {} is rate-limited (try again at {})",
            agent_kind.as_str(),
            recovery
        );
        if let Some(next_agent) = args.cascade.first() {
            eprintln!("[aid] Switching to cascade agent: {}", next_agent);
        } else if let Some(suggested) = crate::agent::selection::coding_fallback_for(&agent_kind) {
            eprintln!(
                "[aid] Suggested fallback: --cascade {} (similar capability)",
                suggested.as_str()
            );
        } else {
            eprintln!("[aid] Tip: use --cascade <agent> or --agent with `aid retry`");
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
    let agent: Box<dyn agent::Agent> = if agent_kind == AgentKind::Custom {
        agent::registry::resolve_custom_agent(custom_agent_name.as_deref().unwrap_or(""))
            .ok_or_else(|| anyhow::anyhow!("Custom agent '{}' not found in registry", args.agent_name))?
    } else {
        agent::get_agent(agent_kind)
    };
    let task_id = args.existing_task_id.clone().unwrap_or_else(TaskId::generate);
    let log_path = paths::log_path(task_id.as_str());
    let workgroup = run_prompt::load_workgroup(&store, args.group.as_deref())?;
    let explicit_repo_path = args.repo.as_deref().map(run_prompt::resolve_repo_path).transpose()?;
    // Create worktree if requested, override dir to point into it
    let (wt_path, wt_branch, effective_dir, resolved_repo) = run_prompt::resolve_worktree_paths(&args, explicit_repo_path.as_deref())?;
    // Use resolved repo_path (always set when worktree is created, even without --repo)
    let repo_path = resolved_repo.clone().or(explicit_repo_path);
    let caller = session::current_caller();
    let task = Task {
        id: task_id.clone(),
        agent: agent_kind,
        custom_agent_name: custom_agent_name.clone(),
        prompt: args.prompt.clone(),
        resolved_prompt: None,
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
        read_only: args.read_only,
        budget: args.budget,
    };
    let dispatch_warnings = validate_dispatch(&args, &agent_kind);
    for warning in &dispatch_warnings {
        eprintln!("[aid] Warning: {warning}");
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
    let opts = RunOpts {
        dir: effective_dir.clone(),
        output: args.output.clone(),
        model: effective_model.clone(),
        budget: budget_active,
        read_only: args.read_only,
        context_files: prompt_bundle.context_files,
        session_id: args.session_id.clone(),
    };
    let mut runtime_hooks = hooks::load_hooks()?;
    runtime_hooks.extend(hooks::parse_cli_hooks(&args.hooks)?);
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
                agent_display_name,
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
            && let Some(target_dir) = agent::target_dir_for_worktree(args.worktree.as_deref())
        {
            tokio_cmd.env("CARGO_TARGET_DIR", &target_dir);
        }
        tokio_cmd.stdout(std::process::Stdio::piped());
        tokio_cmd.stderr(std::process::Stdio::piped());
        if args.announce {
            println!(
                "Task {} started ({}: {})",
                task_id,
                agent_display_name,
                crate::agent::truncate::truncate_text(&args.prompt, 50)
            );
        }
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
        )
        .await?;
        run_prompt::warn_agent_committed_files_outside_scope(
            &args.scope,
            args.dir.as_ref(),
            effective_dir.as_ref(),
            resolved_repo.as_ref(),
            wt_path.as_ref(),
        );
        // Detect worktree escape: warn if agent modified files in main repo
        if args.worktree.is_some() {
            check_worktree_escape(repo_path.as_deref());
        }
        let pre_verify_status =
            store.get_task(task_id.as_str())?.map(|task| task.status).unwrap_or(TaskStatus::Done);
        maybe_verify(
            &store,
            &task_id,
            args.verify.as_deref(),
            effective_dir.as_deref(),
        );
        if !args.scope.is_empty() {
            check_scope_violations(&store, &task_id, &args.scope, effective_dir.as_deref());
        }
        if let Some(task) = store.get_task(task_id.as_str())? {
            if task.status == TaskStatus::Done && !prompt_bundle.injected_memory_ids.is_empty() {
                for memory_id in &prompt_bundle.injected_memory_ids {
                    if let Err(err) = store.increment_memory_success(memory_id) {
                        eprintln!("[aid] Failed to record memory success for {memory_id}: {err}");
                    }
                }
            }
            maybe_flag_empty_worktree_diff(store.as_ref(), &task_id, &task);
            maybe_cleanup_fast_fail(&store, &task_id, &task);
            // Auto-cleanup worktree for failed tasks (no useful changes to preserve)
            if task.status == TaskStatus::Failed {
                let fail_payload = show::task_hook_json(
                    &task_id,
                    agent_display_name,
                    TaskStatus::Failed,
                    &task.prompt,
                    task.worktree_path.as_deref(),
                    effective_dir.as_deref(),
                    task.exit_code,
                );
                if let Err(err) = hooks::run_hooks_with(
                    "on_fail",
                    &fail_payload,
                    Some(agent_display_name),
                    &runtime_hooks,
                    false,
                ) {
                    eprintln!("[aid] Hook on_fail failed: {err}");
                }
                if let Some(wt) = task.worktree_path.as_deref()
                    && std::path::Path::new(wt).exists()
                {
                    let repo = repo_path.as_deref().unwrap_or(".");
                    crate::cmd::merge::remove_worktree(repo, wt);
                }
            }
        }
        if let Some(retry_id) = maybe_judge_retry(&store, &args, &task_id).await? {
            return Ok(retry_id);
        }
        if let Some(ref reviewer_agent) = args.peer_review
            && let Some(task) = store.get_task(task_id.as_str())?
            && task.status == TaskStatus::Done
        {
            match judge::peer_review_task(&task, reviewer_agent, &args.prompt).await {
                Ok(review) => {
                    eprintln!(
                        "[aid] Peer review by {reviewer_agent}: {}/10 — {}",
                        review.score, review.feedback
                    );
                    store.save_peer_review(
                        task_id.as_str(),
                        reviewer_agent,
                        review.score,
                        &review.feedback,
                    )?;
                }
                Err(e) => eprintln!("[aid] Peer review failed: {e}"),
            }
        }
        run_prompt::notify_task_completion(&store, &task_id)?;
        let summary = crate::cmd::summary::generate_summary(&store.get_task(task_id.as_str())?.unwrap());
        let summary_json = serde_json::to_string(&summary).unwrap_or_default();
        let _ = store.save_completion_summary(task_id.as_str(), &summary_json);
        if let Some(task) = store.get_task(task_id.as_str())? {
            let done_payload = show::task_hook_json(
                &task_id,
                agent_display_name,
                task.status,
                &task.prompt,
                task.worktree_path.as_deref(),
                effective_dir.as_deref(),
                task.exit_code,
            );
            if let Err(err) = hooks::run_hooks_with(
                "after_complete",
                &done_payload,
                Some(agent_display_name),
                &runtime_hooks,
                false,
            ) {
                eprintln!("[aid] Hook after_complete failed: {err}");
            }
        }
        crate::webhook::fire_task_webhooks(&store, task_id.as_str()).await;
        if args.announce {
            let status_hint = if let Some(task) = store.get_task(task_id.as_str())? {
                match task.status {
                    TaskStatus::Done => {
                        format!("[aid] Next: aid show {task_id} --diff | aid merge {task_id}")
                    }
                    TaskStatus::Failed => {
                        let base = format!(
                            "[aid] Next: aid show {task_id} | aid retry {task_id} -f \"feedback\""
                        );
                        if task.duration_ms.unwrap_or(i64::MAX) < 5000 {
                            let stderr = retry_logic::read_stderr_tail(task_id.as_str(), 3);
                            format!("{base}\n[aid] Hint: task failed in <5s — check agent binary is installed and --dir points to a valid repo\n[aid] stderr: {stderr}")
                        } else {
                            base
                        }
                    }
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
        if let Some(mut retry_args) = retry_logic::prepare_retry(store.clone(), &task_id, &args).await? {
            if let Some(task) = store.get_task(task_id.as_str())? {
                inherit_retry_base_branch(args.dir.as_deref(), &task, &mut retry_args);
            }
            Box::pin(run(store, retry_args)).await?;
        } else if let Some(task) = store.get_task(task_id.as_str())?
            && task.status == TaskStatus::Failed
            && let Some((next_agent, remaining_cascade)) = take_next_cascade_agent(&args)
        {
            eprintln!(
                "[aid] Cascade: trying {} after {} failed",
                next_agent,
                args.agent_name
            );
            let mut cascade_args = args.clone();
            cascade_args.agent_name = next_agent;
            cascade_args.cascade = remaining_cascade;
            cascade_args.parent_task_id = Some(task_id.as_str().to_string());
            Box::pin(run(store, cascade_args)).await?;
        }
    }
    Ok(task_id)
}

fn maybe_flag_empty_worktree_diff(store: &Store, task_id: &TaskId, task: &Task) {
    if task.status != TaskStatus::Done || task.verify_status != VerifyStatus::Skipped {
        return;
    }
    let Some(wt_path) = task.worktree_path.as_deref() else {
        return;
    };
    let path = Path::new(wt_path);
    if !path.exists() {
        return;
    }
    if let Some(true) = worktree_is_empty_diff(path) {
        eprintln!("[aid] Warning: agent completed but made no code changes in worktree");
        if let Err(err) = store.update_verify_status(task_id.as_str(), VerifyStatus::EmptyDiff) {
            eprintln!("[aid] Failed to record empty diff status: {err}");
        }
    }
}
fn take_next_cascade_agent(args: &RunArgs) -> Option<(String, Vec<String>)> {
    let mut cascade = args.cascade.clone();
    if cascade.is_empty() {
        None
    } else {
        let next_agent = cascade.remove(0);
        Some((next_agent, cascade))
    }
}

fn worktree_is_empty_diff(worktree_dir: &Path) -> Option<bool> {
    let head = git_diff_stat_output(worktree_dir, &["diff", "--stat", "HEAD"])?;
    let staged = git_diff_stat_output(worktree_dir, &["diff", "--cached", "--stat"])?;
    Some(head.trim().is_empty() && staged.trim().is_empty())
}

fn git_diff_stat_output(dir: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn empty_diff_detection_respects_worktree_state() {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init"]);
        git(dir.path(), &["config", "user.email", "aid@example.com"]);
        git(dir.path(), &["config", "user.name", "Aid Tester"]);
        let file = dir.path().join("file.txt");
        std::fs::write(&file, "initial").unwrap();
        git(dir.path(), &["add", "file.txt"]);
        git(dir.path(), &["commit", "-m", "initial"]);

        assert_eq!(worktree_is_empty_diff(dir.path()), Some(true));

        std::fs::write(&file, "updated").unwrap();

        assert_eq!(worktree_is_empty_diff(dir.path()), Some(false));
    }

    #[test]
    fn take_next_cascade_agent_consumes_first_entry() {
        let args = RunArgs {
            agent_name: "primary".to_string(),
            cascade: vec!["codex".to_string(), "cursor".to_string()],
            ..Default::default()
        };
        let result = take_next_cascade_agent(&args);
        assert_eq!(result, Some(("codex".to_string(), vec!["cursor".to_string()])));
    }

    #[test]
    fn take_next_cascade_agent_returns_none_when_empty() {
        let args = RunArgs { cascade: vec![], ..Default::default() };
        assert!(take_next_cascade_agent(&args).is_none());
    }

    #[test]
    fn validate_dispatch_warns_short_prompt() {
        assert_eq!(validate_dispatch(&RunArgs { prompt: "tiny".to_string(), ..Default::default() }, &AgentKind::Gemini), vec!["Prompt is very short, agent may not have enough context".to_string()]);
    }
    #[test]
    fn validate_dispatch_warns_code_agent_without_dir() {
        assert_eq!(validate_dispatch(&RunArgs { prompt: "adequate prompt".to_string(), ..Default::default() }, &AgentKind::Codex), vec!["Code agent without --dir may not be able to write files".to_string()]);
    }
    #[test]
    fn validate_dispatch_warns_long_prompt() {
        let prompt = "a".repeat(5001);
        assert_eq!(validate_dispatch(&RunArgs { prompt, ..Default::default() }, &AgentKind::Gemini), vec!["Very long prompt (5001 chars), consider using --context files instead".to_string()]);
    }
    #[test]
    fn validate_dispatch_warns_research_worktree() {
        assert_eq!(validate_dispatch(&RunArgs { prompt: "valid prompt text".to_string(), worktree: Some("wt".to_string()), ..Default::default() }, &AgentKind::Gemini), vec!["Research agent with --worktree is unusual, did you mean a code agent?".to_string()]);
    }

}


pub(crate) fn inherit_retry_base_branch(repo_dir: Option<&str>, task: &Task, retry_args: &mut RunArgs) { run_prompt::inherit_retry_base_branch_impl(repo_dir, task, retry_args); }
pub(crate) use run_agent::run_agent_process;

pub(crate) fn maybe_cleanup_fast_fail(store: &Store, task_id: &TaskId, task: &Task) { run_prompt::maybe_cleanup_fast_fail_impl(store, task_id, task); }
/// Run verification if --verify was set and a working dir exists.
pub(crate) fn maybe_verify(store: &Store, task_id: &TaskId, verify: Option<&str>, dir: Option<&str>) { run_prompt::maybe_verify_impl(store, task_id, verify, dir); }
pub(crate) async fn maybe_auto_retry_after_verify_failure(store: &Arc<Store>, task_id: &TaskId, args: &RunArgs, pre_verify_status: TaskStatus) -> Result<Option<TaskId>> {
    run_prompt::maybe_auto_retry_after_verify_failure_impl(store, task_id, args, pre_verify_status).await
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
    eprintln!(
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
