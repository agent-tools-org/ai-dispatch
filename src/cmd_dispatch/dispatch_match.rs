// aid CLI command matching.
// Routes Commands variants to focused handler functions.

use super::{handlers_a, handlers_b, handlers_c};
use crate::cli::{Commands, command_args_a, command_args_b, command_args_c};
use anyhow::Result;
use std::sync::Arc;

pub(crate) async fn dispatch(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    match command {
        command @ (
            Commands::Run(..)
            | Commands::Batch(..)
            | Commands::Benchmark(..)
            | Commands::Watch(..)
            | Commands::Board(..)
            | Commands::Completions
            | Commands::Changelog(..)
            | Commands::Agent(..)
            | Commands::Clean(..)
            | Commands::Show(..)
            | Commands::Export(..)
            | Commands::Tree(..)
            | Commands::Output(..)
            | Commands::Usage(..)
            | Commands::Cost(..)
            | Commands::Stats(..)
            | Commands::Summary(..)
        ) => dispatch_primary(store, command).await,
        command @ (
            Commands::Retry(..)
            | Commands::Merge(..)
            | Commands::Respond(..)
            | Commands::Reply(..)
            | Commands::Stop(..)
            | Commands::Kill(..)
            | Commands::Steer(..)
            | Commands::Unstick(..)
            | Commands::Ask(..)
            | Commands::Query(..)
            | Commands::Mcp
            | Commands::Hook(..)
            | Commands::Config(..)
            | Commands::Group(..)
            | Commands::Container(..)
            | Commands::Worktree(..)
            | Commands::Store(..)
            | Commands::Team(..)
            | Commands::Tool(..)
            | Commands::Doctor(..)
            | Commands::Credential(..)
        ) => dispatch_secondary(store, command).await,
        command @ (
            Commands::Project(..)
            | Commands::Memory(..)
            | Commands::Kg(..)
            | Commands::Finding(..)
            | Commands::Broadcast(..)
            | Commands::Upgrade(..)
            | Commands::Init
            | Commands::Setup
            | Commands::InternalRunTask(..)
            | Commands::Experiment(..)
        ) => dispatch_tertiary(store, command).await,
        #[cfg(feature = "web")]
        Commands::Web(command_args_c::WebArgs { port }) => handlers_b::run_web(port).await,
    }
}

async fn dispatch_primary(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    match command {
        Commands::Run(command_args_a::RunArgs { agent, prompt, prompt_file, repo, repo_root, dir, output, result_file, model, budget, no_hint, worktree, team, group, verify, iterate, eval, eval_feedback_template, judge, peer_review, retry, context, checklist, checklist_file, scope, run_extras, no_skill, bg, dry_run, read_only, sandbox, container, best_of, metric, parent, id, timeout, idle_timeout, audit, no_audit, no_link_deps }) => handlers_a::run(store, agent, prompt, prompt_file, repo, repo_root, dir, output, result_file, model, budget, no_hint, worktree, team, group, verify, iterate, eval, eval_feedback_template, judge, peer_review, retry, context, checklist, checklist_file, scope, run_extras, no_skill, bg, dry_run, read_only, sandbox, container, best_of, metric, parent, id, timeout, idle_timeout, audit, no_audit, no_link_deps).await,
        Commands::Batch(command_args_a::BatchArgs { action, file, vars, group, repo_root, parallel, analyze, wait, dry_run, no_prompt, yes, force, max_concurrent, output }) => handlers_a::batch(store, action, file, vars, parallel, analyze, wait, dry_run, no_prompt, yes, force, max_concurrent, output, group, repo_root).await,
        Commands::Benchmark(command_args_a::BenchmarkArgs { prompt, agents, dir, verify }) => handlers_c::benchmark(store, prompt, agents, dir, verify).await,
        Commands::Watch(command_args_a::WatchArgs { task_ids, group, tui, quiet, stream, exit_on_await, timeout }) => handlers_c::watch(store, task_ids, group, tui, quiet, stream, exit_on_await, timeout).await,
        Commands::Board(command_args_a::BoardArgs { running, today, mine, group, limit, force, stream, json }) => {
            handlers_c::board(store, running, today, mine, group, limit, force, stream, json).await
        }
        Commands::Completions => handlers_c::completions(),
        Commands::Changelog(command_args_a::ChangelogArgs { version, all, count, git }) => handlers_c::changelog(version, all, count, git),
        Commands::Agent(command_args_a::AgentArgs { action }) => handlers_c::agent(action),
        Commands::Clean(command_args_a::CleanArgs { older_than, worktrees, dry_run }) => handlers_c::clean(store, older_than, worktrees, dry_run),
        Commands::Show(command_args_a::ShowArgs { task_id, events: _, context, diff, summary, file, output, result, transcript, full, brief, explain, log, json, agent, model }) => handlers_c::show(store, task_id, context, diff, summary, file, output, result, transcript, full, brief, explain, log, json, agent, model).await,
        Commands::Export(command_args_b::ExportArgs { task_id, format, sharegpt, output }) => {
            handlers_b::export(store, task_id, format, sharegpt, output).await
        }
        Commands::Tree(command_args_c::TreeArgs { task_id }) => handlers_b::tree(store, task_id),
        Commands::Output(command_args_c::OutputArgs { task_id, full: _, brief }) => handlers_b::output(task_id, brief),
        Commands::Usage(command_args_b::UsageArgs { session, agent, team, period, json }) => handlers_b::usage(store, session, agent, team, period, json),
        Commands::Cost(command_args_b::CostArgs { group, summary, agent, period }) => handlers_b::cost(store, group, summary, agent, period),
        Commands::Stats(command_args_b::StatsArgs { window, agent, insights }) => crate::cmd::stats::run(&store, window, agent, insights),
        Commands::Summary(command_args_b::SummaryArgs { group }) => handlers_b::summary(store, group),
        _ => unreachable!("dispatch_primary received unsupported command"),
    }
}

async fn dispatch_secondary(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    match command {
        Commands::Retry(command_args_b::RetryArgs { task_id, feedback, agent, dir, reset }) => handlers_b::retry(store, task_id, feedback, agent, dir, reset).await,
        Commands::Merge(command_args_b::MergeArgs { task_id, group, approve, check, force, target, lanes }) => {
            handlers_b::merge(store, task_id, group, approve, check, force, target, lanes)
        }
        Commands::Respond(command_args_b::RespondArgs { task_id, input, file }) => handlers_b::respond(task_id, input, file),
        Commands::Reply(command_args_b::ReplyArgs { task_id, message, file, async_mode, timeout_secs }) => {
            handlers_b::reply(store, task_id, message, file, async_mode, timeout_secs)
        }
        Commands::Stop(command_args_b::StopArgs { task_id, force, retry_tree }) => {
            handlers_b::stop(store, task_id, force, retry_tree)
        }
        Commands::Kill(command_args_b::KillArgs { task_id }) => handlers_b::kill(store, task_id),
        Commands::Steer(command_args_b::SteerArgs { task_id, message }) => handlers_b::steer(store, task_id, message),
        Commands::Unstick(command_args_b::UnstickArgs { task_id, message, escalate }) => {
            handlers_b::unstick(store, task_id, message, escalate)
        }
        Commands::Ask(command_args_b::AskArgs { prompt, agent, model, files, output }) => handlers_b::ask(store, prompt, agent, model, files, output).await,
        Commands::Query(command_args_b::QueryArgs { prompt, auto, model, group, finding }) => handlers_b::query(store, prompt, auto, model, group, finding),
        Commands::Mcp => handlers_b::mcp(store).await,
        Commands::Hook(command_args_b::HookArgs { action }) => handlers_b::hook(action),
        Commands::Config(command_args_b::ConfigArgs { action }) => handlers_b::config(store, action),
        Commands::Group(command_args_b::GroupArgs { action }) => handlers_b::group(store, action),
        Commands::Container(command_args_b::ContainerArgs { action }) => handlers_b::container(action),
        Commands::Worktree(command_args_c::WorktreeArgs { action }) => handlers_b::worktree(action),
        Commands::Store(command_args_c::StoreArgs { action }) => handlers_b::store(action),
        Commands::Team(command_args_c::TeamArgs { action }) => handlers_b::team(action),
        Commands::Tool(command_args_c::ToolArgs { action }) => handlers_b::tool(action),
        Commands::Doctor(command_args_c::DoctorArgs { apply }) => handlers_c::doctor(store, apply),
        Commands::Credential(command_args_c::CredentialArgs { action }) => handlers_b::credential(action),
        _ => unreachable!("dispatch_secondary received unsupported command"),
    }
}

async fn dispatch_tertiary(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    match command {
        Commands::Project(command_args_c::ProjectArgs { action }) => handlers_b::project(action),
        Commands::Memory(command_args_c::MemoryArgs { action }) => handlers_b::memory(store, action),
        Commands::Kg(command_args_c::KgArgs { action }) => handlers_b::kg(store, action),
        Commands::Finding(command_args_c::FindingArgs { action }) => handlers_b::finding(store, action),
        Commands::Broadcast(command_args_c::BroadcastArgs { group, message }) => handlers_b::broadcast(store, group, message),
        Commands::Upgrade(command_args_c::UpgradeArgs { force }) => handlers_b::upgrade(force),
        Commands::Init => handlers_b::init(),
        Commands::Setup => handlers_b::setup(),
        Commands::InternalRunTask(command_args_c::InternalRunTaskArgs { task_id }) => handlers_b::internal_run_task(store, task_id).await,
        Commands::Experiment(subcommand) => handlers_b::experiment(store, subcommand).await,
        _ => unreachable!("dispatch_tertiary received unsupported command"),
    }
}
