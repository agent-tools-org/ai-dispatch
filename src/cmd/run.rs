// Handler for `aid run <agent> <prompt>` — dispatch a task to an AI CLI.
// Creates task record, spawns agent process, wires watcher, records completion.

use anyhow::{Result, Context};
use chrono::Local;
use std::sync::Arc;
use tokio::process::Command;

use crate::agent::{self, RunOpts};
use crate::cost;
use crate::paths;
use crate::store::Store;
use crate::types::*;
use crate::watcher;

pub struct RunArgs {
    pub agent_name: String,
    pub prompt: String,
    pub dir: Option<String>,
    pub output: Option<String>,
    pub model: Option<String>,
    pub worktree: Option<String>,
    pub verify: Option<String>,
    pub context: Vec<String>,
    pub background: bool,
}

pub async fn run(store: Arc<Store>, args: RunArgs) -> Result<()> {
    let agent_kind = AgentKind::from_str(&args.agent_name)
        .ok_or_else(|| anyhow::anyhow!(
            "Unknown agent '{}'. Available: gemini, codex, opencode",
            args.agent_name
        ))?;

    let agent = agent::get_agent(agent_kind);
    let task_id = TaskId::generate();
    let log_path = paths::log_path(task_id.as_str());

    // Create worktree if requested, override dir to point into it
    let (wt_path, wt_branch, effective_dir) = if let Some(ref branch) = args.worktree {
        let repo_dir = args.dir.as_deref().unwrap_or(".");
        let info = crate::worktree::create_worktree(
            std::path::Path::new(repo_dir), branch,
        )?;
        let p = info.path.to_string_lossy().to_string();
        (Some(p.clone()), Some(info.branch), Some(p))
    } else {
        (None, None, args.dir.clone())
    };

    let task = Task {
        id: task_id.clone(),
        agent: agent_kind,
        prompt: args.prompt.clone(),
        status: TaskStatus::Pending,
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

    // Inject context into prompt if specified
    let effective_prompt = if !args.context.is_empty() {
        let specs = crate::context::parse_context_specs(&args.context)?;
        let ctx = crate::context::resolve_context(&specs)?;
        crate::context::inject_context(&args.prompt, &ctx)
    } else {
        args.prompt.clone()
    };

    let opts = RunOpts {
        dir: effective_dir.clone(),
        output: args.output.clone(),
        model: args.model.clone(),
    };

    // Build the OS command via the agent adapter
    let std_cmd = agent.build_command(&effective_prompt, &opts)
        .context("Failed to build agent command")?;

    // Convert std::process::Command to tokio::process::Command
    let mut tokio_cmd = Command::from(std_cmd);
    tokio_cmd.stdout(std::process::Stdio::piped());
    tokio_cmd.stderr(std::process::Stdio::piped());

    if args.background {
        // Background mode: spawn and return immediately
        store.update_task_status(task_id.as_str(), TaskStatus::Running)?;
        let store_bg = store.clone();
        let task_id_bg = task_id.clone();
        let is_streaming = agent.streaming();
        let model = args.model.clone();
        let verify_bg = args.verify.clone();
        let dir_bg = effective_dir.clone();

        println!("Task {} started in background ({}: {})",
            task_id, agent_kind, truncate(&args.prompt, 50));

        tokio::spawn(async move {
            let result = run_agent_process(
                &*agent, tokio_cmd, &task_id_bg, &store_bg, &log_path,
                args.output.as_deref(), model.as_deref(), is_streaming,
            ).await;
            match result {
                Err(e) => {
                    eprintln!("Background task {} failed: {}", task_id_bg, e);
                    let _ = store_bg.update_task_status(task_id_bg.as_str(), TaskStatus::Failed);
                }
                Ok(()) => {
                    maybe_verify(&store_bg, &task_id_bg, verify_bg.as_deref(), dir_bg.as_deref());
                }
            }
        });
    } else {
        // Foreground mode: run and wait
        store.update_task_status(task_id.as_str(), TaskStatus::Running)?;
        println!("Task {} started ({}: {})",
            task_id, agent_kind, truncate(&args.prompt, 50));

        let is_streaming = agent.streaming();
        run_agent_process(
            &*agent, tokio_cmd, &task_id, &store, &log_path,
            args.output.as_deref(), args.model.as_deref(), is_streaming,
        ).await?;

        maybe_verify(&store, &task_id, args.verify.as_deref(), effective_dir.as_deref());
    }

    Ok(())
}

async fn run_agent_process(
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
    let mut child = cmd.spawn()
        .context("Failed to spawn agent process")?;

    let info = if streaming {
        watcher::watch_streaming(agent, &mut child, task_id, store, log_path).await?
    } else {
        let out = output_path.map(std::path::Path::new);
        watcher::watch_buffered(agent, &mut child, task_id, store, log_path, out).await?
    };

    let duration_ms = start.elapsed().as_millis() as i64;
    let cost_usd = info.tokens
        .and_then(|t| cost::estimate_cost(t, model, agent.kind()));
    store.update_task_completion(
        task_id.as_str(),
        info.status,
        info.tokens,
        duration_ms,
        model,
        cost_usd,
    )?;

    // Print summary
    let duration_str = format_duration(duration_ms);
    let tokens_str = info.tokens
        .map(|t| format!(", {} tokens", t))
        .unwrap_or_default();
    let cost_str = if cost_usd.is_some() {
        format!(", {}", cost::format_cost(cost_usd))
    } else {
        String::new()
    };
    println!("Task {} {} ({}{}{})", task_id, info.status.label(), duration_str, tokens_str, cost_str);

    Ok(())
}

fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 { format!("{secs}s") }
    else { format!("{}m {:02}s", secs / 60, secs % 60) }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() }
    else { format!("{}...", &s[..max.saturating_sub(3)]) }
}

/// Run verification if --verify was set and a working dir exists.
fn maybe_verify(store: &Store, task_id: &TaskId, verify: Option<&str>, dir: Option<&str>) {
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
