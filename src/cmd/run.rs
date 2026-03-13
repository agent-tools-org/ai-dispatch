// Handler for `aid run <agent> <prompt>` — dispatch a task to an AI CLI.
// Creates task record, spawns agent process, wires watcher, records completion.

use anyhow::{Context, Result};
use chrono::Local;
use std::sync::Arc;
use tokio::process::Command;
use tokio::time::{Duration, sleep};

use crate::agent::{self, RunOpts};
use crate::background::{self, BackgroundRunSpec};
use crate::cost;
use crate::paths;
use crate::session;
use crate::store::Store;
use crate::types::*;
use crate::watcher;

#[derive(Clone)]
pub struct RunArgs {
    pub agent_name: String,
    pub prompt: String,
    pub dir: Option<String>,
    pub output: Option<String>,
    pub model: Option<String>,
    pub worktree: Option<String>,
    pub group: Option<String>,
    pub verify: Option<String>,
    pub retry: u32,
    pub context: Vec<String>,
    pub background: bool,
    pub parent_task_id: Option<String>,
}

pub async fn run(store: Arc<Store>, args: RunArgs) -> Result<TaskId> {
    let agent_kind = AgentKind::parse_str(&args.agent_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown agent '{}'. Available: gemini, codex, opencode, cursor",
            args.agent_name
        )
    })?;

    let agent = agent::get_agent(agent_kind);
    let task_id = TaskId::generate();
    let log_path = paths::log_path(task_id.as_str());
    let workgroup = load_workgroup(&store, args.group.as_deref())?;

    // Create worktree if requested, override dir to point into it
    let (wt_path, wt_branch, effective_dir) = if let Some(ref branch) = args.worktree {
        let repo_dir = args.dir.as_deref().unwrap_or(".");
        let info = crate::worktree::create_worktree(std::path::Path::new(repo_dir), branch)?;
        let p = info.path.to_string_lossy().to_string();
        (Some(p.clone()), Some(info.branch), Some(p))
    } else {
        (None, None, args.dir.clone())
    };

    let caller = session::current_caller();
    let task = Task {
        id: task_id.clone(),
        agent: agent_kind,
        prompt: args.prompt.clone(),
        status: TaskStatus::Pending,
        parent_task_id: args.parent_task_id.clone(),
        workgroup_id: args.group.clone(),
        caller_kind: caller.as_ref().map(|item| item.kind.clone()),
        caller_session_id: caller.as_ref().map(|item| item.session_id.clone()),
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
    let effective_prompt = crate::workgroup::compose_prompt(
        &args.prompt,
        file_context.as_deref(),
        workgroup.as_ref(),
    );

    let opts = RunOpts {
        dir: effective_dir.clone(),
        output: args.output.clone(),
        model: args.model.clone(),
    };

    if args.background {
        store.update_task_status(task_id.as_str(), TaskStatus::Running)?;
        let spec = BackgroundRunSpec {
            task_id: task_id.as_str().to_string(),
            agent_name: agent_kind.as_str().to_string(),
            prompt: effective_prompt,
            dir: effective_dir,
            output: args.output.clone(),
            model: args.model.clone(),
            verify: args.verify.clone(),
            retry: args.retry,
            group: args.group.clone(),
        };
        background::save_spec(&spec)?;
        if let Err(err) = background::spawn_worker(task_id.as_str()) {
            store.update_task_status(task_id.as_str(), TaskStatus::Failed)?;
            return Err(err);
        }

        println!(
            "Task {} started in background ({}: {})",
            task_id,
            agent_kind,
            truncate(&args.prompt, 50)
        );
    } else {
        let std_cmd = agent
            .build_command(&effective_prompt, &opts)
            .context("Failed to build agent command")?;
        let mut tokio_cmd = Command::from(std_cmd);
        tokio_cmd.stdout(std::process::Stdio::piped());
        tokio_cmd.stderr(std::process::Stdio::piped());
        store.update_task_status(task_id.as_str(), TaskStatus::Running)?;
        println!(
            "Task {} started ({}: {})",
            task_id,
            agent_kind,
            truncate(&args.prompt, 50)
        );

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
        retry_if_needed(store.clone(), &task_id, &args).await?;
    }

    Ok(task_id)
}

pub(crate) async fn retry_if_needed(
    store: Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
) -> Result<()> {
    if args.retry == 0 {
        return Ok(());
    }

    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(());
    };
    if task.status != TaskStatus::Failed {
        return Ok(());
    }

    let stderr_tail = read_stderr_tail(task_id.as_str(), 5);
    if let Some(parent_id) = args.parent_task_id.as_deref()
        && stderr_tail == read_stderr_tail(parent_id, 5)
    {
        println!("Retry stopped: identical stderr to previous attempt.");
        return Ok(());
    }

    let depth = retry_depth(&store, args.parent_task_id.as_deref())?;
    let attempt = depth + 1;
    let max_attempts = depth + args.retry;
    let backoff_secs = backoff_for_attempt(attempt);
    println!("Retry {attempt}/{max_attempts}: re-dispatching after {backoff_secs}s...");
    sleep(Duration::from_secs(backoff_secs)).await;
    let original_prompt = root_prompt(&store, &task).unwrap_or_else(|| args.prompt.clone());

    let retry_prompt = format!(
        "[Previous attempt failed]\nError: {stderr_tail}\n\n[Original task]\n{prompt}",
        prompt = original_prompt,
    );
    let mut retry_args = args.clone();
    retry_args.prompt = retry_prompt;
    retry_args.retry = args.retry.saturating_sub(1);
    retry_args.background = false;
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    Box::pin(run(store, retry_args)).await?;
    Ok(())
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
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

    let command = if verify_arg == "auto" {
        None
    } else {
        Some(verify_arg)
    };
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

fn read_stderr_tail(task_id: &str, lines: usize) -> String {
    let stderr_path = paths::stderr_path(task_id);
    let Ok(stderr) = std::fs::read_to_string(stderr_path) else {
        return "stderr unavailable".to_string();
    };
    let tail: Vec<&str> = stderr.lines().rev().take(lines).collect();
    if tail.is_empty() {
        "stderr unavailable".to_string()
    } else {
        tail.into_iter().rev().collect::<Vec<_>>().join("\n")
    }
}

fn retry_depth(store: &Store, parent_task_id: Option<&str>) -> Result<u32> {
    let mut depth = 0u32;
    let mut current = parent_task_id.map(str::to_string);
    while let Some(task_id) = current {
        let Some(task) = store.get_task(&task_id)? else {
            break;
        };
        depth += 1;
        current = task.parent_task_id;
    }
    Ok(depth)
}

fn backoff_for_attempt(attempt: u32) -> u64 {
    match attempt {
        0 | 1 => 5,
        2 => 15,
        _ => 45,
    }
}

fn root_prompt(store: &Store, task: &Task) -> Option<String> {
    let mut prompt = task.prompt.clone();
    let mut current = task.parent_task_id.clone();
    while let Some(task_id) = current {
        let Some(parent) = store.get_task(&task_id).ok().flatten() else {
            break;
        };
        prompt = parent.prompt;
        current = parent.parent_task_id;
    }
    Some(prompt)
}

fn load_workgroup(store: &Store, group_id: Option<&str>) -> Result<Option<Workgroup>> {
    let Some(group_id) = group_id else {
        return Ok(None);
    };
    store
        .get_workgroup(group_id)?
        .ok_or_else(|| anyhow::anyhow!("Workgroup '{}' not found", group_id))
        .map(Some)
}
