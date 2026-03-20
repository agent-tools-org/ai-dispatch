// aid CLI display-oriented dispatch handlers.
// Implements benchmark, watch, board, show, and related wrappers.

use super::resolve_group;
use crate::cli::AgentCommands;
use crate::cmd;
use crate::{notify, store, tui};
use anyhow::Result;
use std::sync::Arc;

pub(super) async fn benchmark(
    store: Arc<store::Store>,
    prompt: String,
    agents: String,
    dir: Option<String>,
    verify: Option<String>,
) -> Result<()> {
    cmd::benchmark::run(store, prompt, agents, dir, verify).await
}

pub(super) async fn watch(
    store: Arc<store::Store>,
    task_ids: Vec<String>,
    group: Option<String>,
    tui_enabled: bool,
    quiet: bool,
    exit_on_await: bool,
    timeout: Option<u64>,
) -> Result<()> {
    let group = resolve_group(group);
    if tui_enabled {
        tui::run(&store, tui::RunOptions { task_id: task_ids.first().cloned(), group })?;
    } else if quiet {
        cmd::wait::run(&store, &task_ids, group.as_deref(), exit_on_await, timeout).await?;
    } else {
        cmd::watch::run(&store, &task_ids, group.as_deref(), quiet, exit_on_await).await?;
    }
    Ok(())
}

pub(super) async fn board(
    store: Arc<store::Store>,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<String>,
    stream: bool,
    json: bool,
) -> Result<()> {
    let group = resolve_group(group);
    if stream {
        cmd::board_stream::run(&store, running, today, mine, group.as_deref()).await?;
    } else {
        cmd::board::run(&store, running, today, mine, group.as_deref(), json)?;
    }
    Ok(())
}

pub(super) fn completions() -> Result<()> {
    let text = notify::read_recent(20)?;
    if !text.is_empty() {
        println!("{text}");
    }
    Ok(())
}

pub(super) fn changelog(version: Option<String>, all: bool, count: usize, git: bool) -> Result<()> {
    cmd::changelog::run(version, all, count, git)
}

pub(super) fn agent(action: AgentCommands) -> Result<()> {
    use cmd::agent::{AgentAction, run_agent_command};
    let action = match action {
        AgentCommands::List => AgentAction::List,
        AgentCommands::Show { name } => AgentAction::Show { name },
        AgentCommands::Config { name, model } => AgentAction::Config { name, model },
        AgentCommands::Add { name } => AgentAction::Add { name },
        AgentCommands::Remove { name } => AgentAction::Remove { name },
        AgentCommands::Fork { name, new_name } => AgentAction::Fork { name, new_name },
        AgentCommands::Quota => AgentAction::Quota,
    };
    run_agent_command(action)
}

pub(super) fn clean(store: Arc<store::Store>, older_than: u64, worktrees: bool, dry_run: bool) -> Result<()> {
    cmd::clean::run(store, older_than, worktrees, dry_run)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn show(
    store: Arc<store::Store>,
    task_id: String,
    context: bool,
    diff: bool,
    summary: bool,
    file: Option<String>,
    output: bool,
    full: bool,
    explain: bool,
    log: bool,
    json: bool,
    agent: Option<String>,
    model: Option<String>,
) -> Result<()> {
    cmd::show::run(
        store,
        cmd::show::ShowArgs { task_id, context, diff, summary, file, output, full, explain, log, json, agent, model },
    )
    .await
}
