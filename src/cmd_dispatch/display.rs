// aid CLI display-oriented dispatch handlers.
// Implements benchmark, watch, board, show, output, export, and related wrappers.

use super::resolve_group;
use crate::cli::AgentCommands;
use crate::cmd;
use crate::{notify, store, tui};
use anyhow::Result;
use std::sync::Arc;

pub(super) async fn export(
    store: Arc<store::Store>,
    task_id: String,
    format: String,
    sharegpt: bool,
    output: Option<String>,
) -> Result<()> {
    let format = cmd::export::ExportFormat::parse(&format)?;
    cmd::export::run(store, cmd::export::ExportArgs { task_id, format, sharegpt, output }).await
}

pub(super) fn tree(store: Arc<store::Store>, task_id: String) -> Result<()> {
    cmd::tree::run(&store, &task_id)
}

pub(super) fn output(task_id: String, brief: bool) -> Result<()> {
    let store = store::Store::open(&crate::paths::db_path())?;
    let text = cmd::show::output_text_for_task(&store, &task_id, !brief)?;
    print!("{text}");
    Ok(())
}

pub(super) fn usage(
    store: Arc<store::Store>,
    session: bool,
    agent: Option<String>,
    team: Option<String>,
    period: String,
    json: bool,
) -> Result<()> {
    cmd::usage::run(&store, session, agent, team, period, json)
}

pub(super) fn cost(
    store: Arc<store::Store>,
    group: Option<String>,
    summary: bool,
    agent: Option<String>,
    period: String,
) -> Result<()> {
    cmd::cost::run(&store, group, summary, agent, period)
}

pub(super) fn summary(store: Arc<store::Store>, group: String) -> Result<()> {
    cmd::summary_cli::run(&store, &group)
}

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
    wait: bool,
    stream: bool,
    exit_on_await: bool,
    timeout: Option<u64>,
) -> Result<()> {
    let group = resolve_group(group);
    if tui_enabled {
        tui::run(&store, tui::RunOptions { task_id: task_ids.first().cloned(), group })?;
    } else if stream {
        cmd::watch_stream::run(&store, &task_ids, group.as_deref(), timeout).await?;
    } else if wait {
        cmd::wait::run(&store, &task_ids, group.as_deref(), exit_on_await, timeout).await?;
    } else {
        cmd::watch::run(&store, &task_ids, group.as_deref(), exit_on_await, timeout).await?;
    }
    Ok(())
}

pub(super) async fn wait(
    store: Arc<store::Store>,
    task_ids: Vec<String>,
    group: Option<String>,
    exit_on_await: bool,
    timeout: Option<u64>,
) -> Result<()> {
    let group = resolve_group(group);
    cmd::wait::run(&store, &task_ids, group.as_deref(), exit_on_await, timeout).await
}

pub(super) async fn board(
    store: Arc<store::Store>,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<String>,
    limit: Option<usize>,
    force: bool,
    stream: bool,
    json: bool,
) -> Result<()> {
    let group = resolve_group(group);
    if stream {
        cmd::board_stream::run(&store, running, today, mine, group.as_deref(), limit).await?;
    } else {
        cmd::board::run(&store, running, today, mine, group.as_deref(), limit, force, json)?;
    }
    Ok(())
}

pub(super) fn notifications() -> Result<()> {
    let text = notify::read_recent(20)?;
    if !text.is_empty() {
        println!("{text}");
    }
    Ok(())
}

pub(super) fn changelog(version: Option<String>, all: bool, count: usize, git: bool) -> Result<()> {
    cmd::changelog::run(version, all, count, git)
}

pub(super) fn agent(store: Arc<store::Store>, action: AgentCommands) -> Result<()> {
    use cmd::agent::{AgentAction, run_agent_command};
    let action = match action {
        AgentCommands::List { json } => AgentAction::List { json },
        AgentCommands::Show { name, json } => AgentAction::Show { name, json },
        AgentCommands::Config { name, model, idle_timeout, disable, enable } => {
            AgentAction::Config { name, model, idle_timeout, disable, enable }
        }
        AgentCommands::Add { name } => AgentAction::Add { name },
        AgentCommands::Remove { name } => AgentAction::Remove { name },
        AgentCommands::Fork { name, new_name } => AgentAction::Fork { name, new_name },
        AgentCommands::Quota => AgentAction::Quota,
    };
    run_agent_command(&store, action)
}

pub(super) fn clean(store: Arc<store::Store>, older_than: u64, worktrees: bool, dry_run: bool) -> Result<()> {
    cmd::clean::run(store, older_than, worktrees, dry_run)
}

pub(super) fn doctor(store: Arc<store::Store>, apply: bool) -> Result<()> {
    cmd::doctor::run(&store, apply)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn show(
    store: Arc<store::Store>,
    task_id: String,
    events: bool,
    context: bool,
    diff: bool,
    summary: bool,
    file: Option<String>,
    branch: bool,
    output: bool,
    result: bool,
    transcript: bool,
    full: bool,
    brief: bool,
    explain: bool,
    log: bool,
    json: bool,
    agent: Option<String>,
    model: Option<String>,
) -> Result<()> {
    cmd::show::run(
        store,
        cmd::show::ShowArgs {
            task_id,
            events,
            context,
            diff,
            summary,
            file,
            branch,
            output,
            result,
            transcript,
            full,
            brief,
            explain,
            log,
            json,
            agent,
            model,
        },
    )
    .await
}
