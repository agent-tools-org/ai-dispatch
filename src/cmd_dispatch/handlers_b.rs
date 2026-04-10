// aid CLI secondary dispatch handlers.
// Implements group, memory, finding, and remaining command wrappers.

use super::{resolve_finding_content, resolve_group};
use crate::cli::{
    ExperimentCommands, FindingCommands, HookAction, KgCommands, MemoryCommands, StoreCommands,
};
use crate::cli_actions::{ConfigAction, ContainerAction, CredentialAction, GroupAction, GroupFindingAction, ProjectAction, TeamAction, ToolAction, WorktreeAction};
use crate::cmd;
use crate::cmd::experiment_types::{ExperimentConfig, MetricDirection};
use crate::{background, cli, store};
use anyhow::{Result, anyhow};
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
pub(super) async fn retry(
    store: Arc<store::Store>,
    task_id: String,
    feedback: String,
    agent: Option<String>,
    dir: Option<String>,
    reset: bool,
) -> Result<()> {
    cmd::retry::run(store, cmd::retry::RetryArgs { task_id, feedback, agent, dir, reset })
        .await
        .map(|_| ())
}
pub(super) fn merge(
    store: Arc<store::Store>,
    task_id: Option<String>,
    group: Option<String>,
    approve: bool,
    check: bool,
    target: Option<String>,
    lanes: bool,
) -> Result<()> {
    if lanes && group.is_none() {
        return Err(anyhow!("--lanes requires --group"));
    }
    let group = resolve_group(group);
    cmd::merge::run(store, task_id.as_deref(), group.as_deref(), approve, check, target.as_deref(), lanes)
}
pub(super) fn respond(task_id: String, input: Option<String>, file: Option<String>) -> Result<()> {
    cmd::respond::run(&task_id, input.as_deref(), file.as_deref())
}
pub(super) fn stop(store: Arc<store::Store>, task_id: String, force: bool) -> Result<()> {
    if force {
        cmd::stop::kill(&store, &task_id)
    } else {
        cmd::stop::stop(&store, &task_id)
    }
}
pub(super) fn kill(store: Arc<store::Store>, task_id: String) -> Result<()> {
    cmd::stop::kill(&store, &task_id)
}
pub(super) fn steer(store: Arc<store::Store>, task_id: String, message: String) -> Result<()> {
    cmd::steer::run(&store, &task_id, &message)
}
pub(super) async fn ask(
    store: Arc<store::Store>,
    prompt: String,
    agent: Option<String>,
    model: Option<String>,
    files: Vec<String>,
    output: Option<String>,
) -> Result<()> {
    cmd::ask::run(store, prompt, agent, model, files, output).await
}
pub(super) fn query(
    store: Arc<store::Store>,
    prompt: String,
    auto: bool,
    model: Option<String>,
    group: Option<String>,
    finding: bool,
) -> Result<()> {
    let group = group.or_else(|| resolve_group(None));
    cmd::query::run(&store, &prompt, model.as_deref(), auto, group.as_deref(), finding)
}
pub(super) async fn mcp(store: Arc<store::Store>) -> Result<()> {
    cmd::mcp::run(store).await
}
pub(super) fn hook(action: HookAction) -> Result<()> {
    match action {
        HookAction::SessionStart => cmd::hook::session_start(),
    }
}

pub(super) fn config(store: Arc<store::Store>, action: ConfigAction) -> Result<()> {
    cmd::config::run(&store, action)
}
pub(super) fn group(store: Arc<store::Store>, action: GroupAction) -> Result<()> {
    match action {
        GroupAction::Create { name, context, id } => {
            cmd::group::create(&store, &name, context.as_deref().unwrap_or(""), id.as_deref())
        }
        GroupAction::List => cmd::group::list(&store),
        GroupAction::Show { group_id } => cmd::group::show(&store, &group_id),
        GroupAction::Update { group_id, name, context } => {
            cmd::group::update(&store, &group_id, name.as_deref(), context.as_deref())
        }
        GroupAction::Delete { group_id } => cmd::group::delete(&store, &group_id),
        GroupAction::Cancel { group_id } => cmd::group::cancel(&store, &group_id),
        GroupAction::Summary { group_id } => cmd::summary_cli::run(&store, &group_id),
        GroupAction::Finding { action } => group_finding(store, action),
        GroupAction::Broadcast { group_id, message } => cmd::broadcast::run(&store, &group_id, &message),
    }
}

fn group_finding(store: Arc<store::Store>, action: GroupFindingAction) -> Result<()> {
    match action {
        GroupFindingAction::Add { group, content, stdin, file, task, severity, title, finding_file, lines, category, confidence } => {
            let content = resolve_finding_content(content, stdin, file)?;
            cmd::finding::add(&store, &group, &content, task.as_deref(), severity.as_deref(), title.as_deref(), finding_file.as_deref(), lines.as_deref(), category.as_deref(), confidence.as_deref())
        }
        GroupFindingAction::List { group, json, count, severity, verdict } => cmd::finding::list(&store, &group, json, count, severity.as_deref(), verdict.as_deref()),
        GroupFindingAction::Get { group, finding_id, json } => cmd::finding::get(&store, &group, finding_id, json),
        GroupFindingAction::Update { group, finding_id, verdict, score, note } => cmd::finding::update(&store, &group, finding_id, verdict.as_deref(), score.as_deref(), note.as_deref()),
    }
}

pub(super) fn worktree(action: WorktreeAction) -> Result<()> {
    match action {
        WorktreeAction::Create { branch, base, repo } => cmd::worktree::create(&branch, base.as_deref(), repo.as_deref()),
        WorktreeAction::List { repo } => cmd::worktree::list(repo.as_deref()),
        WorktreeAction::Prune { repo } => cmd::worktree::prune(repo.as_deref()),
        WorktreeAction::Remove { branch, repo } => cmd::worktree::remove(&branch, repo.as_deref()),
    }
}

pub(super) fn container(action: ContainerAction) -> Result<()> {
    use cmd::container::{ContainerAction as ContainerCommand, run_container_command};
    let action = match action {
        ContainerAction::Build { tag, file } => ContainerCommand::Build { tag, file },
        ContainerAction::List => ContainerCommand::List,
        ContainerAction::Stop { name } => ContainerCommand::Stop { name },
    };
    run_container_command(action)
}

pub(super) fn store(action: StoreCommands) -> Result<()> {
    use cmd::store::{StoreAction, run_store};
    let action = match action {
        StoreCommands::Browse { query } => StoreAction::Browse { query },
        StoreCommands::Install { name } => StoreAction::Install { name },
        StoreCommands::Show { name } => StoreAction::Show { name },
        StoreCommands::Update { apply } => StoreAction::Update { apply },
    };
    run_store(action)
}

pub(super) fn team(action: TeamAction) -> Result<()> {
    use cmd::team::{TeamAction as TeamCommand, run_team_command};
    let action = match action {
        TeamAction::List => TeamCommand::List,
        TeamAction::Show { name } => TeamCommand::Show { name },
        TeamAction::Create { name } => TeamCommand::Create { name },
        TeamAction::Delete { name } => TeamCommand::Delete { name },
    };
    run_team_command(action)
}

pub(super) fn tool(action: ToolAction) -> Result<()> {
    cmd::tool::run_tool_command(action)
}

pub(super) fn credential(action: CredentialAction) -> Result<()> {
    use cmd::credential::{CredentialAction as CredentialCommand, run_credential_command};
    let action = match action {
        CredentialAction::List => CredentialCommand::List,
        CredentialAction::Add { provider, name, env } => CredentialCommand::Add { provider, name, env },
        CredentialAction::Remove { provider, name } => CredentialCommand::Remove { provider, name },
    };
    run_credential_command(action)
}

pub(super) fn project(action: ProjectAction) -> Result<()> {
    use cmd::project::{ProjectAction as ProjectCommand, run_project_command};
    let action = match action {
        ProjectAction::Init => ProjectCommand::Init,
        ProjectAction::Show => ProjectCommand::Show,
        ProjectAction::State => ProjectCommand::State,
        ProjectAction::Sync => ProjectCommand::Sync,
    };
    run_project_command(action)
}

pub(super) fn memory(store: Arc<store::Store>, action: MemoryCommands) -> Result<()> {
    match action {
        MemoryCommands::Add { memory_type, content, tier, project } => {
            cmd::memory::add(&store, &memory_type, tier.as_deref(), &content, project.as_deref())
        }
        MemoryCommands::List { memory_type, all, project, stats } => {
            cmd::memory::list(&store, memory_type.as_deref(), project.as_deref(), all, stats)
        }
        MemoryCommands::Search { query, project } => {
            cmd::memory::search(&store, &query, project.as_deref())
        }
        MemoryCommands::Update { id, content } => cmd::memory::update(&store, &id, &content),
        MemoryCommands::Forget { id } => cmd::memory::forget(&store, &id),
        MemoryCommands::History { id } => cmd::memory::history(&store, &id),
    }
}

pub(super) fn kg(store: Arc<store::Store>, action: KgCommands) -> Result<()> {
    match action {
        KgCommands::Add { subject, predicate, object, valid_from, source } => {
            cmd::kg::add(&store, &subject, &predicate, &object, valid_from.as_deref(), source.as_deref())
        }
        KgCommands::Query { entity, as_of } => cmd::kg::query(&store, &entity, as_of.as_deref()),
        KgCommands::Invalidate { id } => cmd::kg::invalidate(&store, id),
        KgCommands::Timeline { entity } => cmd::kg::timeline(&store, &entity),
        KgCommands::Search { query } => cmd::kg::search(&store, &query),
        KgCommands::Stats => cmd::kg::stats(&store),
    }
}

pub(super) fn finding(store: Arc<store::Store>, action: FindingCommands) -> Result<()> {
    match action {
        FindingCommands::Add { group, content, stdin, file, task, severity, title, finding_file, lines, category, confidence } => {
            let content = resolve_finding_content(content, stdin, file)?;
            cmd::finding::add(&store, &group, &content, task.as_deref(), severity.as_deref(), title.as_deref(), finding_file.as_deref(), lines.as_deref(), category.as_deref(), confidence.as_deref())
        }
        FindingCommands::List { group, json, count, severity, verdict } => cmd::finding::list(&store, &group, json, count, severity.as_deref(), verdict.as_deref()),
        FindingCommands::Get { group, finding_id, json } => cmd::finding::get(&store, &group, finding_id, json),
        FindingCommands::Update { group, finding_id, verdict, score, note } => cmd::finding::update(&store, &group, finding_id, verdict.as_deref(), score.as_deref(), note.as_deref()),
    }
}

pub(super) fn broadcast(store: Arc<store::Store>, group: String, message: String) -> Result<()> {
    cmd::broadcast::run(&store, &group, &message)
}

pub(super) fn upgrade(force: bool) -> Result<()> {
    cmd::upgrade::run(force)
}

#[cfg(feature = "web")]
pub(super) async fn run_web(port: u16) -> Result<()> {
    cmd::web::run(port).await
}

pub(super) fn init() -> Result<()> {
    cmd::init::run()
}

pub(super) fn setup() -> Result<()> {
    cmd::setup::run()
}

pub(super) async fn internal_run_task(store: Arc<store::Store>, task_id: String) -> Result<()> {
    background::run_task(store, &task_id).await
}

pub(super) async fn experiment(store: Arc<store::Store>, subcommand: ExperimentCommands) -> Result<()> {
    match subcommand {
        cli::ExperimentCommands::Run { agent, prompt, metric, direction, checks, max_runs, worktree, verify } => {
            let config = ExperimentConfig {
                metric_command: metric,
                direction: if direction.eq_ignore_ascii_case("min") { MetricDirection::Min } else { MetricDirection::Max },
                agent,
                prompt,
                checks,
                max_runs: Some(max_runs),
                worktree,
                verify,
            };
            cmd::experiment::run_experiment(store, config).await
        }
        cli::ExperimentCommands::Status { dir } => cmd::experiment_status::run_status(dir.as_deref()),
    }
}
