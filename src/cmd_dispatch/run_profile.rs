// Declared-profile validation and agent/model resolution for `aid run`.
// Exports: validate_task_profile(), resolve_run_agent().
// Deps: selection advice, routing hints, config/team/store, task-profile types.

use std::sync::Arc;

use anyhow::{Result, anyhow};

use crate::agent;
use crate::agent::classifier::TaskCategory;
use crate::cmd_dispatch::recommend_hint;
use crate::store;
use crate::team;
use crate::types::{AgentKind, DeclaredTaskProfile, TaskBudget, TaskDifficulty, TaskRigor, TaskUrgency};

pub(super) fn validate_task_profile(
    difficulty: Option<TaskDifficulty>,
    budget: Option<TaskBudget>,
    urgency: Option<TaskUrgency>,
    rigor: Option<TaskRigor>,
) -> Result<()> {
    let missing = [
        difficulty.is_none().then_some("difficulty"),
        budget.is_none().then_some("budget"),
        urgency.is_none().then_some("urgency"),
        rigor.is_none().then_some("rigor"),
    ].into_iter().flatten().collect::<Vec<_>>();
    if missing.is_empty() { return Ok(()) }
    let project = crate::project::detect_project();
    if project.as_ref().is_some_and(|item| item.require_task_profile) {
        anyhow::bail!("Task profile is required; missing --{}", missing.join(", --"));
    }
    aid_warn!(
        "[aid] Warning: task profile incomplete (missing --{}); undeclared values will be stored as null",
        missing.join(", --")
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_run_agent(
    store: &Arc<store::Store>, prompt: &str, dir: &Option<String>, repo: &Option<String>,
    output: &Option<String>, result_file: &Option<String>, model: &Option<String>, budget: bool,
    difficulty: Option<TaskDifficulty>, declared_budget: Option<TaskBudget>,
    urgency: Option<TaskUrgency>, rigor: Option<TaskRigor>, kind: Option<TaskCategory>,
    no_hint: bool, read_only: bool, sandbox: bool, worktree: &Option<String>,
    team_flag: &Option<String>, agent_name: String,
) -> Result<(String, Option<String>)> {
    let selection_opts = agent::RunOpts {
        dir: dir.clone().or_else(|| repo.clone())
            .or_else(|| worktree.as_ref().map(|_| ".".to_string())),
        output: output.clone(), result_file: result_file.clone(), model: model.clone(),
        budget, read_only, sandbox, context_files: vec![], session_id: None,
        env: None, env_forward: None,
    };
    let team_config = team_flag.as_deref().and_then(team::resolve_team);
    if agent_name != "auto" {
        recommend_hint::emit_if_recommended(
            &agent_name, prompt, no_hint, &selection_opts, store, team_config.as_ref(),
        );
        return explicit_agent(agent_name, model, declared_budget, rigor);
    }
    let declared = DeclaredTaskProfile {
        difficulty: difficulty.unwrap_or_default(),
        budget: declared_budget.unwrap_or_else(|| {
            if budget { TaskBudget::Cheap } else { TaskBudget::Standard }
        }),
        urgency: urgency.unwrap_or_default(),
        rigor: rigor.unwrap_or_default(),
    };
    let advice = agent::selection::advise(
        prompt, declared, kind, team_config.as_ref(), Some(store.as_ref()), 0,
    );
    let recommended = advice.recommended
        .ok_or_else(|| anyhow!("No eligible agent for the declared task profile"))?;
    aid_info!("[aid] Auto-selected: {} (reason: {})", recommended.agent, recommended.reason);
    let recommended_model = model.is_none().then_some(recommended.model).flatten();
    Ok((recommended.agent, recommended_model))
}

fn explicit_agent(
    agent_name: String,
    model: &Option<String>,
    declared_budget: Option<TaskBudget>,
    rigor: Option<TaskRigor>,
) -> Result<(String, Option<String>)> {
    let selected_kind = AgentKind::parse_str(&agent_name);
    if rigor == Some(TaskRigor::Critical) && !is_local_trust(&agent_name, selected_kind) {
        anyhow::bail!("Agent '{agent_name}' is not eligible for --rigor critical (local trust required)");
    }
    let selected_model = if model.is_none() {
        selected_kind.and_then(|kind| agent::selection::model_for_task_budget(
            kind, declared_budget.unwrap_or(TaskBudget::Standard),
        )).map(str::to_string)
    } else {
        None
    };
    if model.is_none()
        && matches!(declared_budget, Some(TaskBudget::Free | TaskBudget::Cheap))
        && selected_model.is_none()
        && selected_kind != Some(AgentKind::Antigravity)
    {
        anyhow::bail!("Agent '{agent_name}' has no model eligible for the declared budget");
    }
    Ok((agent_name, selected_model))
}

fn is_local_trust(agent_name: &str, kind: Option<AgentKind>) -> bool {
    if let Some(kind) = kind {
        return kind.profile().is_some_and(|profile| profile.5 == "local");
    }
    agent::registry::load_custom_agents().get(agent_name)
        .is_some_and(|config| config.trust_tier == "local")
}
