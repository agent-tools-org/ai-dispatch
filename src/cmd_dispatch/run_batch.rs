// aid CLI run and batch dispatch handlers.
// Implements run and batch command wrappers.
#[path = "run_batch_args.rs"]
mod run_batch_args;
#[path = "run_profile.rs"]
mod run_profile;

use crate::cli::{BatchAction, RunExtrasArgs};
use crate::cmd;
use crate::types::TaskId;
use crate::types::{TaskBudget, TaskDifficulty, TaskEgress, TaskRigor, TaskUrgency};
use crate::agent::classifier::TaskCategory;
use crate::{config, store};
use anyhow::{Context, Result, anyhow};
use std::sync::Arc;

use self::run_batch_args::build_run_args;
use self::run_profile::{resolve_run_agent, validate_task_profile};

#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    store: Arc<store::Store>,
    agent_name: String,
    prompt: Option<String>,
    prompt_file: Option<String>,
    repo: Option<String>,
    repo_root: Option<String>,
    dir: Option<String>,
    output: Option<String>,
    result_file: Option<String>,
    model: Option<String>,
    difficulty: Option<TaskDifficulty>,
    budget: Option<TaskBudget>,
    urgency: Option<TaskUrgency>,
    rigor: Option<TaskRigor>,
    egress: TaskEgress,
    kind: Option<TaskCategory>,
    no_hint: bool,
    worktree: Option<String>,
    team_flag: Option<String>,
    group: Option<String>,
    verify: Option<String>,
    iterate: Option<u32>,
    eval: Option<String>,
    eval_feedback_template: Option<String>,
    judge: Option<String>,
    peer_review: Option<String>,
    retry: u32,
    context: Vec<String>,
    checklist: Vec<String>,
    checklist_file: Option<String>,
    scope: Vec<String>,
    run_extras: Box<RunExtrasArgs>,
    no_skill: bool,
    bg: bool,
    dry_run: bool,
    read_only: bool,
    sandbox: bool,
    container: Option<String>,
    best_of: Option<usize>,
    metric: Option<String>,
    parent: Option<String>,
    id: Option<String>,
    timeout: Option<u64>,
    idle_timeout: Option<u64>,
    audit: bool,
    no_audit: bool,
    no_link_deps: bool,
) -> Result<TaskId> {
    validate_task_profile(difficulty, budget, urgency, rigor)?;
    let config = config::load_config().unwrap_or_default();
    let budget_mode = budget.is_some_and(TaskBudget::uses_budget_mode)
        || config.selection.budget_mode;
    let selection_prompt = match (&prompt, prompt_file.as_deref()) {
        (Some(prompt), _) if !prompt.is_empty() => prompt.clone(),
        (_, Some(file)) => std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read prompt file: {file}"))?,
        _ => String::new(),
    };
    let (agent_name, auto_model) = resolve_run_agent(
        &store,
        &selection_prompt,
        &dir,
        &repo,
        &output,
        &result_file,
        &model,
        budget_mode,
        difficulty,
        budget,
        urgency,
        rigor,
        egress,
        kind,
        no_hint,
        read_only,
        sandbox,
        &worktree,
        &team_flag,
        agent_name,
    )?;
    let checklist = cmd::checklist::merge_checklist_items(checklist, checklist_file.as_deref())?;
    let args = build_run_args(
        agent_name,
        prompt.unwrap_or_default(),
        prompt_file,
        repo,
        repo_root,
        dir,
        output,
        result_file,
        model,
        auto_model,
        worktree,
        group,
        verify,
        iterate,
        eval,
        eval_feedback_template,
        judge,
        peer_review,
        retry,
        context,
        checklist,
        scope,
        run_extras,
        no_skill,
        bg,
        dry_run,
        read_only,
        sandbox,
        container,
        budget_mode,
        difficulty,
        budget,
        urgency,
        rigor,
        egress,
        kind,
        best_of,
        metric,
        team_flag,
        parent,
        id,
        timeout,
        idle_timeout,
        audit,
        no_audit,
        no_link_deps,
    );
    cmd::run::run(store, args).await
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
