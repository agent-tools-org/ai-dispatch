// aid CLI project, worktree, and internal execution dispatch handlers.
// Implements project/worktree commands, experiment runs, and internal task execution.

use crate::cli::{self, ExperimentCommands};
use crate::cli_actions::{ProjectAction, WorktreeAction};
use crate::cmd;
use crate::cmd::experiment_types::{ExperimentConfig, MetricDirection};
use crate::{background, store};
use anyhow::Result;
use std::sync::Arc;

pub(super) fn worktree(action: WorktreeAction) -> Result<()> {
    match action {
        WorktreeAction::Create { branch, base, repo } => cmd::worktree::create(&branch, base.as_deref(), repo.as_deref()),
        WorktreeAction::List { repo, json, active } => cmd::worktree::list(repo.as_deref(), json, active),
    }
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
