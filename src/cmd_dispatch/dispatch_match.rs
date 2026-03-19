// aid CLI command matching.
// Routes Commands variants to focused handler functions.

use super::{handlers_a, handlers_b, handlers_c};
use crate::cli::Commands;
use anyhow::Result;
use std::sync::Arc;

pub(crate) async fn dispatch(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    match command {
        command @ (
            Commands::Run { .. }
            | Commands::Batch { .. }
            | Commands::Benchmark { .. }
            | Commands::Watch { .. }
            | Commands::Board { .. }
            | Commands::Completions
            | Commands::Changelog { .. }
            | Commands::Agent { .. }
            | Commands::Clean { .. }
            | Commands::Show { .. }
            | Commands::Export { .. }
            | Commands::Tree { .. }
            | Commands::Output { .. }
            | Commands::Usage { .. }
            | Commands::Cost { .. }
            | Commands::Summary { .. }
        ) => dispatch_primary(store, command).await,
        command @ (
            Commands::Retry { .. }
            | Commands::Merge { .. }
            | Commands::Respond { .. }
            | Commands::Stop { .. }
            | Commands::Kill { .. }
            | Commands::Steer { .. }
            | Commands::Ask { .. }
            | Commands::Query { .. }
            | Commands::Mcp
            | Commands::Hook { .. }
            | Commands::Config { .. }
            | Commands::Group { .. }
            | Commands::Worktree { .. }
            | Commands::Store { .. }
            | Commands::Team { .. }
        ) => dispatch_secondary(store, command).await,
        command @ (
            Commands::Project { .. }
            | Commands::Memory { .. }
            | Commands::Finding { .. }
            | Commands::Broadcast { .. }
            | Commands::Upgrade { .. }
            | Commands::Init
            | Commands::Setup
            | Commands::InternalRunTask { .. }
            | Commands::Experiment(..)
        ) => dispatch_tertiary(store, command).await,
        #[cfg(feature = "web")]
        Commands::Web { port } => handlers_b::run_web(port).await,
    }
}

async fn dispatch_primary(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    match command {
        Commands::Run { agent, prompt, repo, dir, output, model, budget, worktree, team, group, verify, judge, peer_review, retry, context, scope, run_extras, no_skill, bg, dry_run, read_only, sandbox, best_of, metric, parent, id, timeout } => handlers_a::run(store, agent, prompt, repo, dir, output, model, budget, worktree, team, group, verify, judge, peer_review, retry, context, scope, run_extras, no_skill, bg, dry_run, read_only, sandbox, best_of, metric, parent, id, timeout).await,
        Commands::Batch { action, file, vars, parallel, wait, dry_run, max_concurrent, output } => handlers_a::batch(store, action, file, vars, parallel, wait, dry_run, max_concurrent, output).await,
        Commands::Benchmark { prompt, agents, dir, verify } => handlers_c::benchmark(store, prompt, agents, dir, verify).await,
        Commands::Watch { task_ids, group, tui, quiet, exit_on_await, timeout } => handlers_c::watch(store, task_ids, group, tui, quiet, exit_on_await, timeout).await,
        Commands::Board { running, today, mine, group, stream, json } => handlers_c::board(store, running, today, mine, group, stream, json).await,
        Commands::Completions => handlers_c::completions(),
        Commands::Changelog { version, all, count, git } => handlers_c::changelog(version, all, count, git),
        Commands::Agent { action } => handlers_c::agent(action),
        Commands::Clean { older_than, worktrees, dry_run } => handlers_c::clean(store, older_than, worktrees, dry_run),
        Commands::Show { task_id, events: _, context, diff, summary, file, output, full, explain, log, json, agent, model } => handlers_c::show(store, task_id, context, diff, summary, file, output, full, explain, log, json, agent, model).await,
        Commands::Export { task_id, format, output } => handlers_b::export(store, task_id, format, output).await,
        Commands::Tree { task_id } => handlers_b::tree(store, task_id),
        Commands::Output { task_id, full } => handlers_b::output(task_id, full),
        Commands::Usage { session, agent, team, period, json } => handlers_b::usage(store, session, agent, team, period, json),
        Commands::Cost { group, summary, agent, period } => handlers_b::cost(store, group, summary, agent, period),
        Commands::Summary { group } => handlers_b::summary(store, group),
        _ => unreachable!("dispatch_primary received unsupported command"),
    }
}

async fn dispatch_secondary(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    match command {
        Commands::Retry { task_id, feedback, agent, dir, reset } => handlers_b::retry(store, task_id, feedback, agent, dir, reset).await,
        Commands::Merge { task_id, group, approve } => handlers_b::merge(store, task_id, group, approve),
        Commands::Respond { task_id, input, file } => handlers_b::respond(task_id, input, file),
        Commands::Stop { task_id, force } => handlers_b::stop(store, task_id, force),
        Commands::Kill { task_id } => handlers_b::kill(store, task_id),
        Commands::Steer { task_id, message } => handlers_b::steer(store, task_id, message),
        Commands::Ask { prompt, agent, model, files, output } => handlers_b::ask(store, prompt, agent, model, files, output).await,
        Commands::Query { prompt, auto, model, group, finding } => handlers_b::query(store, prompt, auto, model, group, finding),
        Commands::Mcp => handlers_b::mcp(store).await,
        Commands::Hook { action } => handlers_b::hook(action),
        Commands::Config { action } => handlers_b::config(store, action),
        Commands::Group { action } => handlers_b::group(store, action),
        Commands::Worktree { action } => handlers_b::worktree(action),
        Commands::Store { action } => handlers_b::store(action),
        Commands::Team { action } => handlers_b::team(action),
        _ => unreachable!("dispatch_secondary received unsupported command"),
    }
}

async fn dispatch_tertiary(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    match command {
        Commands::Project { action } => handlers_b::project(action),
        Commands::Memory { action } => handlers_b::memory(store, action),
        Commands::Finding { action } => handlers_b::finding(store, action),
        Commands::Broadcast { group, message } => handlers_b::broadcast(store, group, message),
        Commands::Upgrade { force } => handlers_b::upgrade(force),
        Commands::Init => handlers_b::init(),
        Commands::Setup => handlers_b::setup(),
        Commands::InternalRunTask { task_id } => handlers_b::internal_run_task(store, task_id).await,
        Commands::Experiment(subcommand) => handlers_b::experiment(store, subcommand).await,
        _ => unreachable!("dispatch_tertiary received unsupported command"),
    }
}
