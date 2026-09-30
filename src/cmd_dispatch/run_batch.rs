// aid CLI run and batch dispatch handlers.
// Implements run and batch command wrappers.
#[path = "run_profile.rs"]
mod run_profile;

use crate::cli::{BatchAction, RunExtrasArgs, run_args};
use crate::cmd;
use crate::types::TaskId;
use crate::types::TaskBudget;
use crate::agent::model_validation::ModelSource;
use crate::{config, store};
use anyhow::{Context, Result, anyhow};
use std::sync::Arc;

use self::run_profile::{resolve_run_agent, validate_task_profile};

pub(super) async fn run(
    store: Arc<store::Store>,
    cli_args: run_args::RunArgs,
) -> Result<TaskId> {
    let no_hint = cli_args.no_hint;
    let checklist_file = cli_args.checklist_file.clone();
    let mut args = run_args(cli_args);
    validate_task_profile(
        args.declared_difficulty, args.declared_budget, args.declared_urgency, args.declared_rigor,
    )?;
    let config = config::load_config().unwrap_or_default();
    args.budget = args.declared_budget.is_some_and(TaskBudget::uses_budget_mode)
        || config.selection.budget_mode;
    let selection_prompt = match (&args.prompt, args.prompt_file.as_deref()) {
        (prompt, _) if !prompt.is_empty() => prompt.clone(),
        (_, Some(file)) => std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read prompt file: {file}"))?,
        _ => String::new(),
    };
    args.agent_name = resolve_run_agent(
        &store, &selection_prompt, args.declared_difficulty, args.declared_budget,
        args.declared_urgency, args.declared_rigor, args.declared_egress, args.kind,
        no_hint, &args.team, args.agent_name,
    )?;
    args.checklist = cmd::checklist::merge_checklist_items(args.checklist, checklist_file.as_deref())?;
    cmd::run::run(store, args).await
}

fn run_args(cli: run_args::RunArgs) -> cmd::run::RunArgs {
    let run_args::RunArgs {
        agent, prompt, prompt_file, repo, repo_root, dir, output, result_file, model,
        difficulty, budget, urgency, rigor, egress, kind, no_hint: _, worktree, team,
        group, verify, iterate, eval, eval_feedback_template, judge, peer_review, retry,
        context, checklist, checklist_file: _, scope, run_extras, no_skill, bg, dry_run,
        read_only, sandbox, container, best_of, metric, parent, id, timeout, idle_timeout,
        audit, no_audit, no_link_deps,
    } = cli;
    let RunExtrasArgs {
        remote_build, context_from, skill, template, on_done, cascade, hook, backup, no_backup,
    } = *run_extras;
    let skills = if no_skill {
        vec![cmd::run::NO_SKILL_SENTINEL.to_string()]
    } else {
        skill
    };
    let model_source = if model.is_some() {
        ModelSource::UserSupplied
    } else {
        ModelSource::AidResolved
    };
    cmd::run::RunArgs {
        agent_name: agent, prompt: prompt.unwrap_or_default(), prompt_file, repo, repo_root,
        dir, output, result_file, model, model_source,
        declared_difficulty: difficulty, declared_budget: budget,
        declared_urgency: urgency, declared_rigor: rigor, declared_egress: egress, kind,
        worktree, group: super::resolve_group(group), verify, iterate, eval,
        eval_feedback_template, judge, peer_review, retry, context, checklist, scope, skills,
        remote_build, backup, no_backup, hooks: hook, template,
        background: bg, dry_run, announce: true, on_done, cascade, read_only, sandbox, container,
        best_of, metric, team, context_from, parent_task_id: parent,
        idle_timeout_secs: idle_timeout, existing_task_id: id.map(TaskId), timeout,
        audit, audit_explicit: audit, no_audit, link_deps: !no_link_deps,
        ..Default::default()
    }
}

pub(super) async fn batch(
    store: Arc<store::Store>,
    action: Option<BatchAction>,
    file: Option<String>,
    vars: Vec<String>,
    parallel: bool,
    analyze: bool,
    wait: bool,
    dry_run: bool,
    no_prompt: bool,
    yes: bool,
    force: bool,
    max_concurrent: Option<usize>,
    output: Option<String>,
    group: Option<String>,
    repo_root: Option<String>,
) -> Result<()> {
    match action {
        Some(BatchAction::Init) => cmd::batch::init(output.as_deref())?,
        Some(BatchAction::Retry { group_id, agent, include_waiting }) => {
            cmd::batch::retry_failed(store, &group_id, agent.as_deref(), include_waiting).await?;
        }
        None => {
            let file = file.ok_or_else(|| anyhow!("batch file is required"))?;
            cmd::batch::run(
                store,
                cmd::batch::BatchArgs {
                    file,
                    vars,
                    group,
                    repo_root,
                    parallel,
                    analyze,
                    wait,
                    dry_run,
                    no_prompt,
                    yes,
                    force,
                    max_concurrent,
                },
            )
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "run_batch_tests.rs"]
mod tests;
