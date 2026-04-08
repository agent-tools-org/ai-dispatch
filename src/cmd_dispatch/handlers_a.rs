// aid CLI primary dispatch handlers.
// Implements run, watch, show, and related command wrappers.

use crate::cli::{BatchAction, RunExtrasArgs};
use crate::cmd;
use crate::types::AgentKind;
use crate::{agent, config, store, team};
use anyhow::{Context, Result, anyhow};
use std::sync::Arc;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    store: Arc<store::Store>,
    agent_name: String,
    prompt: Option<String>,
    prompt_file: Option<String>,
    repo: Option<String>,
    dir: Option<String>,
    output: Option<String>,
    result_file: Option<String>,
    model: Option<String>,
    budget: bool,
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
) -> Result<()> {
    let config = config::load_config().unwrap_or_default();
    let budget = budget || config.selection.budget_mode;
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
        budget,
        read_only,
        &worktree,
        &team_flag,
        agent_name,
    );
    let checklist = cmd::checklist::merge_checklist_items(checklist, checklist_file.as_deref())?;
    let args = build_run_args(
        agent_name,
        prompt.unwrap_or_default(),
        prompt_file,
        repo,
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
        budget,
        best_of,
        metric,
        team_flag,
        parent,
        id,
        timeout,
        idle_timeout,
    );
    cmd::run::run(store, args).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_run_args(
    agent_name: String,
    prompt: String,
    prompt_file: Option<String>,
    repo: Option<String>,
    dir: Option<String>,
    output: Option<String>,
    result_file: Option<String>,
    model: Option<String>,
    auto_model: Option<String>,
    worktree: Option<String>,
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
    scope: Vec<String>,
    run_extras: Box<RunExtrasArgs>,
    no_skill: bool,
    bg: bool,
    dry_run: bool,
    read_only: bool,
    sandbox: bool,
    container: Option<String>,
    budget: bool,
    best_of: Option<usize>,
    metric: Option<String>,
    team_flag: Option<String>,
    parent: Option<String>,
    id: Option<String>,
    timeout: Option<u64>,
    idle_timeout: Option<u64>,
) -> cmd::run::RunArgs {
    let extras = *run_extras;
    let skills = if no_skill { vec![cmd::run::NO_SKILL_SENTINEL.to_string()] } else { extras.skill };
    let effective_idle_timeout = idle_timeout.or_else(|| crate::agent_config::get_default_idle_timeout(&agent_name));
    let env = crate::idle_timeout::env_with_idle_timeout(None, effective_idle_timeout);

    cmd::run::RunArgs {
        agent_name,
        prompt,
        prompt_file,
        repo,
        dir,
        output,
        result_file,
        model: model.or(auto_model),
        worktree,
        base_branch: None,
        group: super::resolve_group(group),
        verify,
        iterate,
        eval,
        eval_feedback_template,
        judge,
        peer_review,
        max_duration_mins: None,
        retry,
        context,
        checklist,
        skills,
        hooks: extras.hook,
        template: extras.template,
        background: bg,
        dry_run,
        announce: true,
        on_done: extras.on_done,
        cascade: extras.cascade,
        read_only,
        sandbox,
        container,
        budget,
        best_of,
        metric,
        team: team_flag,
        context_from: extras.context_from,
        scope,
        parent_task_id: parent,
        env,
        existing_task_id: id.map(crate::types::TaskId),
        timeout,
        ..Default::default()
    }
}

fn resolve_run_agent(
    store: &Arc<store::Store>,
    prompt: &str,
    dir: &Option<String>,
    repo: &Option<String>,
    output: &Option<String>,
    result_file: &Option<String>,
    model: &Option<String>,
    budget: bool,
    read_only: bool,
    worktree: &Option<String>,
    team_flag: &Option<String>,
    agent_name: String,
) -> (String, Option<String>) {
    if agent_name != "auto" {
        return (agent_name, None);
    }

    let selection_opts = agent::RunOpts {
        dir: dir
            .clone()
            .or_else(|| repo.clone())
            .or_else(|| worktree.as_ref().map(|_| ".".to_string())),
        output: output.clone(),
        result_file: result_file.clone(),
        model: model.clone(),
        budget,
        read_only,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let team_config = team_flag.as_deref().and_then(team::resolve_team);
    let (selected, reason) =
        agent::select_agent_with_reason(prompt, &selection_opts, store, team_config.as_ref());
    aid_info!("[aid] Auto-selected: {selected} (reason: {reason})");

    let recommended = if model.is_none() && !budget {
        let normalized = prompt.trim().to_lowercase();
        let file_count = agent::classifier::count_file_mentions(&normalized);
        let profile = agent::classifier::classify(prompt, file_count, prompt.len());
        AgentKind::parse_str(&selected)
            .and_then(|kind| agent::selection::recommend_model(&kind, &profile.complexity, false))
            .map(str::to_string)
    } else {
        None
    };
    (selected, recommended)
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
    force: bool,
    max_concurrent: Option<usize>,
    output: Option<String>,
    group: Option<String>,
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
                    parallel,
                    analyze,
                    wait,
                    dry_run,
                    force,
                    max_concurrent,
                },
            )
            .await?;
        }
    }
    Ok(())
}
