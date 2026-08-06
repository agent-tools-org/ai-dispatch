// aid CLI command matching.
// Routes Commands variants to focused handler functions.

use super::{
    DispatchOutcome, RunDispatch, admin_config, display, knowledge, project_worktree, run_batch,
    task_ops,
};
use crate::cli::{Commands, command_args_a, command_args_b, command_args_c, command_args_watch};
use anyhow::Result;
use std::sync::Arc;

pub(crate) async fn dispatch(
    store: Arc<crate::store::Store>,
    command: Commands,
) -> Result<DispatchOutcome> {
    match command {
        Commands::Run(args) => dispatch_run(store, args).await.map(DispatchOutcome::Run),
        command @ (
            Commands::Batch(..)
            | Commands::Advise(..)
            | Commands::Benchmark(..)
            | Commands::Watch(..)
            | Commands::Wait(..)
            | Commands::Board(..)
            | Commands::Notifications
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
        ) => dispatch_primary(store, command).await.map(|()| DispatchOutcome::CommandCompleted),
        command @ (
            Commands::Retry(..)
            | Commands::Merge(..)
            | Commands::Accept(..)
            | Commands::Reject(..)
            | Commands::Gc(..)
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
            | Commands::Build(..)
            | Commands::Test(..)
            | Commands::Worktree(..)
            | Commands::Store(..)
            | Commands::Team(..)
            | Commands::Tool(..)
            | Commands::Doctor(..)
            | Commands::Byok(..)
            | Commands::Credential(..)
        ) => dispatch_secondary(store, command).await.map(|()| DispatchOutcome::CommandCompleted),
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
        ) => dispatch_tertiary(store, command).await.map(|()| DispatchOutcome::CommandCompleted),
        #[cfg(feature = "web")]
        Commands::Web(command_args_c::WebArgs { port }) => admin_config::run_web(port)
            .await
            .map(|()| DispatchOutcome::CommandCompleted),
    }
}

async fn dispatch_run(
    store: Arc<crate::store::Store>,
    args: command_args_a::RunArgs,
) -> Result<RunDispatch> {
    let command_args_a::RunArgs { agent, prompt, prompt_file, repo, repo_root, dir, output, result_file, model, difficulty, budget, urgency, rigor, egress, kind, no_hint, worktree, team, group, verify, iterate, eval, eval_feedback_template, judge, peer_review, retry, context, checklist, checklist_file, scope, run_extras, no_skill, bg, dry_run, read_only, sandbox, container, best_of, metric, parent, id, timeout, idle_timeout, audit, no_audit, no_link_deps } = args;
    let task_id = run_batch::run(store, agent, prompt, prompt_file, repo, repo_root, dir, output, result_file, model, difficulty, budget, urgency, rigor, egress, kind, no_hint, worktree, team, group, verify, iterate, eval, eval_feedback_template, judge, peer_review, retry, context, checklist, checklist_file, scope, run_extras, no_skill, bg, dry_run, read_only, sandbox, container, best_of, metric, parent, id, timeout, idle_timeout, audit, no_audit, no_link_deps).await?;
    Ok(RunDispatch::new(task_id, bg, dry_run))
}

async fn dispatch_primary(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    match command {
        Commands::Advise(args) => crate::cmd::advise::run(Some(store.as_ref()), args),
        Commands::Batch(command_args_a::BatchArgs { action, file, vars, group, repo_root, parallel, analyze, wait, dry_run, no_prompt, yes, force, max_concurrent, output }) => run_batch::batch(store, action, file, vars, parallel, analyze, wait, dry_run, no_prompt, yes, force, max_concurrent, output, group, repo_root).await,
        Commands::Benchmark(command_args_a::BenchmarkArgs { prompt, agents, dir, verify }) => display::benchmark(store, prompt, agents, dir, verify).await,
        Commands::Watch(command_args_watch::WatchArgs { task_ids, group, tui, wait, stream, exit_on_await, timeout }) => display::watch(store, task_ids, group, tui, wait, stream, exit_on_await, timeout).await,
        Commands::Wait(command_args_watch::WaitArgs { task_ids, group, exit_on_await, timeout }) => {
            display::wait(store, task_ids, group, exit_on_await, timeout).await
        }
        Commands::Board(command_args_a::BoardArgs { running, today, mine, group, limit, force, stream, json }) => {
            display::board(store, running, today, mine, group, limit, force, stream, json).await
        }
        Commands::Notifications => display::notifications(),
        Commands::Changelog(command_args_a::ChangelogArgs { version, all, count, git }) => display::changelog(version, all, count, git),
        Commands::Agent(command_args_a::AgentArgs { action }) => display::agent(store, action),
        Commands::Clean(command_args_a::CleanArgs { older_than, worktrees, dry_run }) => display::clean(store, older_than, worktrees, dry_run),
        Commands::Show(command_args_a::ShowArgs { task_id, events, context, diff, summary, file, output, result, transcript, full, brief, explain, log, json, agent, model }) => display::show(store, task_id, events, context, diff, summary, file, output, result, transcript, full, brief, explain, log, json, agent, model).await,
        Commands::Export(command_args_b::ExportArgs { task_id, format, sharegpt, output }) => {
            display::export(store, task_id, format, sharegpt, output).await
        }
        Commands::Tree(command_args_c::TreeArgs { task_id }) => display::tree(store, task_id),
        Commands::Output(command_args_c::OutputArgs { task_id, full: _, brief }) => display::output(task_id, brief),
        Commands::Usage(command_args_b::UsageArgs { session, agent, team, period, json }) => display::usage(store, session, agent, team, period, json),
        Commands::Cost(command_args_b::CostArgs { group, summary, agent, period }) => display::cost(store, group, summary, agent, period),
        Commands::Stats(command_args_b::StatsArgs { window, agent, insights }) => crate::cmd::stats::run(&store, window, agent, insights),
        Commands::Summary(command_args_b::SummaryArgs { group }) => display::summary(store, group),
        _ => unreachable!("dispatch_primary received unsupported command"),
    }
}

async fn dispatch_secondary(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    match command {
        Commands::Retry(command_args_b::RetryArgs { task_id, feedback, agent, dir, reset, bg }) => task_ops::retry(store, task_id, feedback, agent, dir, reset, bg).await,
        Commands::Merge(command_args_b::MergeArgs { task_id, group, approve, check, force, target, lanes }) => {
            task_ops::merge(store, task_id, group, approve, check, force, target, lanes)
        }
        Commands::Accept(command_args_b::ArtifactDecisionArgs { task_id }) => {
            task_ops::accept(store, task_id)
        }
        Commands::Reject(command_args_b::ArtifactDecisionArgs { task_id }) => {
            task_ops::reject(store, task_id)
        }
        Commands::Gc(command_args_b::ArtifactGcArgs { task }) => task_ops::gc(store, task),
        Commands::Respond(command_args_b::RespondArgs { task_id, input, file }) => task_ops::respond(task_id, input, file),
        Commands::Reply(command_args_b::ReplyArgs { task_id, message, file, async_mode, timeout_secs }) => {
            task_ops::reply(store, task_id, message, file, async_mode, timeout_secs)
        }
        Commands::Stop(command_args_b::StopArgs { task_id, force, retry_tree }) => {
            task_ops::stop(store, task_id, force, retry_tree)
        }
        Commands::Kill(command_args_b::KillArgs { task_id }) => task_ops::kill(store, task_id),
        Commands::Steer(command_args_b::SteerArgs { task_id, message }) => task_ops::steer(store, task_id, message),
        Commands::Unstick(command_args_b::UnstickArgs { task_id, message, escalate }) => {
            task_ops::unstick(store, task_id, message, escalate)
        }
        Commands::Ask(command_args_b::AskArgs { prompt, agent, model, files, output }) => knowledge::ask(store, prompt, agent, model, files, output).await,
        Commands::Query(command_args_b::QueryArgs { prompt, auto, model, group, finding }) => knowledge::query(store, prompt, auto, model, group, finding),
        Commands::Mcp => admin_config::mcp(store).await,
        Commands::Hook(command_args_b::HookArgs { action }) => admin_config::hook(action),
        Commands::Config(command_args_b::ConfigArgs { action }) => admin_config::config(store, action),
        Commands::Group(command_args_b::GroupArgs { action }) => knowledge::group(store, action),
        Commands::Container(command_args_b::ContainerArgs { action }) => admin_config::container(action),
        Commands::Build(args) => {
            let code = crate::cmd::build::run(store, args).await?;
            std::process::exit(code);
        }
        Commands::Test(args) => {
            let code = crate::cmd::test_cmd::run(store, args).await?;
            std::process::exit(code);
        }
        Commands::Worktree(command_args_c::WorktreeArgs { action }) => project_worktree::worktree(action),
        Commands::Store(command_args_c::StoreArgs { action }) => admin_config::store(action),
        Commands::Team(command_args_c::TeamArgs { action }) => admin_config::team(action),
        Commands::Tool(command_args_c::ToolArgs { action }) => admin_config::tool(action),
        Commands::Doctor(command_args_c::DoctorArgs { apply }) => display::doctor(store, apply),
        Commands::Byok(command_args_c::ByokArgs { action }) => admin_config::byok(action),
        Commands::Credential(command_args_c::CredentialArgs { action }) => admin_config::credential(action),
        _ => unreachable!("dispatch_secondary received unsupported command"),
    }
}

async fn dispatch_tertiary(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    match command {
        Commands::Project(command_args_c::ProjectArgs { action }) => project_worktree::project(action),
        Commands::Memory(command_args_c::MemoryArgs { action }) => knowledge::memory(store, action),
        Commands::Kg(command_args_c::KgArgs { action }) => knowledge::kg(store, action),
        Commands::Finding(command_args_c::FindingArgs { action }) => knowledge::finding(store, action),
        Commands::Broadcast(command_args_c::BroadcastArgs { group, message }) => knowledge::broadcast(store, group, message),
        Commands::Upgrade(command_args_c::UpgradeArgs { force }) => admin_config::upgrade(force),
        Commands::Init => admin_config::init(),
        Commands::Setup => admin_config::setup(),
        Commands::InternalRunTask(command_args_c::InternalRunTaskArgs { task_id }) => project_worktree::internal_run_task(store, task_id).await,
        Commands::Experiment(subcommand) => project_worktree::experiment(store, subcommand).await,
        _ => unreachable!("dispatch_tertiary received unsupported command"),
    }
}
