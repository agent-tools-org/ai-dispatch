// Handler for `aid run <agent> <prompt>` — dispatch a task to an AI CLI.
// Creates task record, spawns agent process, wires watcher, records completion.

use anyhow::{Context, Result};
use chrono::Local;
use std::sync::Arc;
use tokio::process::Command;

use crate::agent::{self, RunOpts};
use crate::background::{self, BackgroundRunSpec};
use crate::cmd::retry_logic;
use crate::cost;
use crate::notify;
use crate::paths;
use crate::session;
use crate::skills;
use crate::store::Store;
use crate::templates;
use crate::types::*;
use crate::watcher;

pub const NO_SKILL_SENTINEL: &str = "__aid_no_skill__";

#[derive(Clone)]
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
}

fn effective_skills(agent_kind: &AgentKind, args: &RunArgs) -> Vec<String> {
    let manual_skills: Vec<String> = args
        .skills
        .iter()
        .filter(|skill| skill.as_str() != NO_SKILL_SENTINEL)
        .cloned()
        .collect();
    if !manual_skills.is_empty()
        || args
            .skills
            .iter()
            .any(|skill| skill.as_str() == NO_SKILL_SENTINEL)
    {
        return manual_skills;
    }
    skills::auto_skills(agent_kind, args.worktree.is_some())
}

fn resolve_repo_path(path: &str) -> Result<String> {
    let out = std::process::Command::new("git")
        .args(["-C", path, "rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to run git")?;
    anyhow::ensure!(out.status.success(), "Not a git repository: {path}");
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn resolve_dir_in_target(base_dir: &str, dir: Option<&str>, repo_dir: Option<&str>) -> String {
    let Some(dir) = dir else { return base_dir.to_string() };
    let dir_path = std::path::Path::new(dir);
    if dir_path == std::path::Path::new(".") {
        return base_dir.to_string();
    }
    if dir_path.is_absolute()
        && let Some(repo_dir) = repo_dir
        && let Ok(relative_dir) = dir_path.strip_prefix(repo_dir)
    {
        return std::path::Path::new(base_dir)
            .join(relative_dir)
            .to_string_lossy()
            .to_string();
    }
    if dir_path.is_absolute() {
        return dir.to_string();
    }
    std::path::Path::new(base_dir)
        .join(dir_path)
        .to_string_lossy()
        .to_string()
}

pub async fn run(store: Arc<Store>, args: RunArgs) -> Result<TaskId> {
    let agent_kind = AgentKind::parse_str(&args.agent_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown agent '{}'. Available: gemini, codex, opencode, cursor",
            args.agent_name
        )
    })?;
    let requested_skills = effective_skills(&agent_kind, &args);
    if args.skills.is_empty() {
        for skill in &requested_skills {
            eprintln!("[aid] Auto-applied skill: {skill}");
        }
    }

    let agent = agent::get_agent(agent_kind);
    let task_id = TaskId::generate();
    let log_path = paths::log_path(task_id.as_str());
    let workgroup = load_workgroup(&store, args.group.as_deref())?;
    let repo_path = args
        .repo
        .as_deref()
        .map(resolve_repo_path)
        .transpose()?;

    // Create worktree if requested, override dir to point into it
    let (wt_path, wt_branch, effective_dir) = if let Some(ref branch) = args.worktree {
        let repo_dir = repo_path
            .clone()
            .unwrap_or(resolve_repo_path(args.dir.as_deref().unwrap_or("."))?);
        let info = crate::worktree::create_worktree(
            std::path::Path::new(&repo_dir),
            branch,
            args.base_branch.as_deref(),
        )?;
        let p = info.path.to_string_lossy().to_string();
        (
            Some(p.clone()),
            Some(info.branch),
            Some(resolve_dir_in_target(&p, args.dir.as_deref(), Some(&repo_dir))),
        )
    } else if let Some(ref repo_dir) = repo_path {
        (
            None,
            None,
            Some(resolve_dir_in_target(repo_dir, args.dir.as_deref(), Some(repo_dir))),
        )
    } else {
        (None, None, args.dir.clone())
    };

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
        repo_path: repo_path.clone(),
        worktree_path: wt_path,
        worktree_branch: wt_branch,
        log_path: Some(log_path.to_string_lossy().to_string()),
        output_path: args.output.clone(),
        tokens: None,
        duration_ms: None,
        model: args.model.clone(),
        cost_usd: None,
        created_at: Local::now(),
        completed_at: None,
    };
    store.insert_task(&task)?;

    let file_context = if !args.context.is_empty() {
        let specs = crate::context::parse_context_specs(&args.context)?;
        Some(crate::context::resolve_context(&specs)?)
    } else {
        None
    };
    let milestones = if let Some(group_id) = args.group.as_deref() {
        store.get_workgroup_milestones(group_id)?
    } else {
        vec![]
    };
    let prompt = if let Some(template) = args.template.as_deref() {
        let template_content = templates::load_template(template)?;
        templates::apply_template(&template_content, &args.prompt)
    } else {
        args.prompt.clone()
    };
    let mut effective_prompt = crate::workgroup::compose_prompt(
        &prompt,
        file_context.as_deref(),
        workgroup.as_ref(),
        &milestones,
    );
    if !requested_skills.is_empty() {
        let skill_text = skills::load_skills(&requested_skills)?;
        effective_prompt = format!("{effective_prompt}\n\n--- Methodology ---\n{skill_text}");
    }
    effective_prompt = templates::inject_milestone_prompt(&effective_prompt);
    store.update_resolved_prompt(task_id.as_str(), &effective_prompt)?;

    let opts = RunOpts {
        dir: effective_dir.clone(),
        output: args.output.clone(),
        model: args.model.clone(),
        budget: false,
    };

    if args.background {
        store.update_task_status(task_id.as_str(), TaskStatus::Running)?;
        let spec = BackgroundRunSpec {
            task_id: task_id.as_str().to_string(),
            worker_pid: None,
            agent_name: agent_kind.as_str().to_string(),
            prompt: effective_prompt,
            dir: effective_dir,
            output: args.output.clone(),
            model: args.model.clone(),
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
                notify_task_completion(&store, &task_id)?;
                return Err(err);
            }
        };
        if let Err(err) = background::update_worker_pid(task_id.as_str(), worker.id()) {
            let _ = worker.kill();
            let _ = background::clear_spec(task_id.as_str());
            store.update_task_status(task_id.as_str(), TaskStatus::Failed)?;
            notify_task_completion(&store, &task_id)?;
            return Err(err);
        }

        if args.announce {
            println!(
                "Task {} started in background ({}: {})",
                task_id,
                agent_kind,
                crate::agent::truncate::truncate_text(&args.prompt, 50)
            );
        }
    } else {
        let std_cmd = agent
            .build_command(&effective_prompt, &opts)
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
            args.model.as_deref(),
            is_streaming,
        )
        .await?;

        maybe_verify(
            &store,
            &task_id,
            args.verify.as_deref(),
            effective_dir.as_deref(),
        );
        notify_task_completion(&store, &task_id)?;
        crate::webhook::fire_task_webhooks(&store, task_id.as_str()).await;
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

pub(crate) fn inherit_retry_base_branch(repo_dir: Option<&str>, task: &Task, retry_args: &mut RunArgs) {
    if retry_args.base_branch.is_some() || retry_args.worktree.is_none() {
        return;
    }
    let Some(branch) = task.worktree_branch.as_deref() else { return };
    if retry_args.worktree.as_deref() == Some(branch) {
        return;
    }
    let repo_dir = std::path::Path::new(
        task.repo_path
            .as_deref()
            .or(retry_args.repo.as_deref())
            .or(repo_dir)
            .unwrap_or("."),
    );
    if let Ok(true) = crate::worktree::branch_has_commits_ahead_of_main(repo_dir, branch) {
        retry_args.base_branch = Some(branch.to_string());
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_agent_process(
    agent: &dyn crate::agent::Agent,
    mut cmd: Command,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
) -> Result<()> {
    let start = std::time::Instant::now();
    let mut child = cmd.spawn().context("Failed to spawn agent process")?;

    let info = if streaming {
        watcher::watch_streaming(agent, &mut child, task_id, store, log_path).await?
    } else {
        let out = output_path.map(std::path::Path::new);
        watcher::watch_buffered(agent, &mut child, task_id, store, log_path, out).await?
    };

    let duration_ms = start.elapsed().as_millis() as i64;
    let final_model = info.model.as_deref().or(model);
    let cost_usd = info.cost_usd.or_else(|| {
        info.tokens
            .and_then(|tokens| cost::estimate_cost(tokens, final_model, agent.kind()))
    });
    store.update_task_completion(
        task_id.as_str(),
        info.status,
        info.tokens,
        duration_ms,
        final_model,
        cost_usd,
    )?;

    // Print summary
    let duration_str = format_duration(duration_ms);
    let tokens_str = info
        .tokens
        .map(|t| format!(", {} tokens", t))
        .unwrap_or_default();
    let cost_str = if cost_usd.is_some() {
        format!(", {}", cost::format_cost(cost_usd))
    } else {
        String::new()
    };
    println!(
        "Task {} {} ({}{}{})",
        task_id,
        info.status.label(),
        duration_str,
        tokens_str,
        cost_str
    );

    Ok(())
}

fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

fn notify_task_completion(store: &Store, task_id: &TaskId) -> Result<()> {
    if let Some(task) = store.get_task(task_id.as_str())? {
        notify::notify_completion(&task);
    }
    Ok(())
}

/// Run verification if --verify was set and a working dir exists.
pub(crate) fn maybe_verify(
    store: &Store,
    task_id: &TaskId,
    verify: Option<&str>,
    dir: Option<&str>,
) {
    let Some(verify_arg) = verify else { return };
    let Some(dir_path) = dir else {
        println!("Verify skipped: no working directory");
        return;
    };

    let command = if verify_arg == "auto" { None } else { Some(verify_arg) };
    let path = std::path::Path::new(dir_path);

    match crate::verify::run_verify(path, command) {
        Ok(result) => {
            let report = crate::verify::format_verify_report(&result);
            println!("{report}");
            if !result.success {
                let _ = store.update_task_status(task_id.as_str(), TaskStatus::Failed);
                let event = TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: chrono::Local::now(),
                    event_kind: EventKind::Error,
                    detail: format!("Verification failed: {}", result.command),
                    metadata: None,
                };
                let _ = store.insert_event(&event);
            }
        }
        Err(e) => {
            eprintln!("Verify error: {e}");
        }
    }
}

fn load_workgroup(store: &Store, group_id: Option<&str>) -> Result<Option<Workgroup>> {
    let Some(group_id) = group_id else { return Ok(None) };
    store
        .get_workgroup(group_id)?
        .ok_or_else(|| anyhow::anyhow!("Workgroup '{}' not found", group_id))
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_args(skills: Vec<String>) -> RunArgs {
        RunArgs {
            agent_name: "codex".to_string(),
            prompt: "prompt".to_string(),
            repo: None,
            dir: None,
            output: None,
            model: None,
            worktree: None,
            base_branch: None,
            group: None,
            verify: None,
            max_duration_mins: None,
            retry: 0,
            context: vec![],
            skills,
            template: None,
            background: false,
            announce: false,
            parent_task_id: None,
            on_done: None,
            fallback: None,
        }
    }

    #[test]
    fn effective_skills_auto_apply_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let dir = crate::paths::aid_dir().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();

        assert_eq!(
            effective_skills(&AgentKind::Codex, &run_args(vec![])),
            vec!["implementer"]
        );
    }

    #[test]
    fn effective_skills_respect_no_skill_sentinel() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let dir = crate::paths::aid_dir().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();

        assert!(
            effective_skills(
                &AgentKind::Codex,
                &run_args(vec![NO_SKILL_SENTINEL.to_string()])
            )
            .is_empty()
        );
    }
}
