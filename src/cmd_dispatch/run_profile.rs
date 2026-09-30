// Declared-profile validation and agent resolution for `aid run`.
// Exports: validate_task_profile(), resolve_run_agent().
// Deps: selection advice, routing hints, config/team/store, task-profile types.

use std::sync::Arc;

use anyhow::Result;

use crate::agent;
use crate::agent::classifier::TaskCategory;
use crate::cmd_dispatch::recommend_hint;
use crate::store;
use crate::team;
use crate::types::{
    DeclaredTaskProfile, TaskBudget, TaskDifficulty, TaskEgress, TaskRigor, TaskUrgency,
};

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
    store: &Arc<store::Store>, prompt: &str, _dir: &Option<String>, _repo: &Option<String>,
    _output: &Option<String>, _result_file: &Option<String>, _model: &Option<String>, _budget: bool,
    difficulty: Option<TaskDifficulty>, declared_budget: Option<TaskBudget>,
    urgency: Option<TaskUrgency>, rigor: Option<TaskRigor>, egress: TaskEgress,
    kind: Option<TaskCategory>, no_hint: bool, _read_only: bool, _sandbox: bool,
    _worktree: &Option<String>, team_flag: &Option<String>, agent_name: String,
) -> Result<String> {
    if agent::selection::is_removed_auto_agent(&agent_name) {
        anyhow::bail!("{}", agent::selection::AUTO_AGENT_REMOVED_MSG);
    }
    let declared = DeclaredTaskProfile {
        difficulty: difficulty.unwrap_or_default(), budget: declared_budget.unwrap_or_default(),
        urgency: urgency.unwrap_or_default(), rigor: rigor.unwrap_or_default(),
    };
    let team_config = team_flag.as_deref().and_then(team::resolve_team);
    if let Some(hint) = recommend_hint::recommendation_hint(
        &agent_name, prompt, no_hint, declared, kind, store, team_config.as_ref(),
    ) {
        aid_hint!("{hint}");
    }
    explicit_agent(agent_name, egress)
}

/// Egress gates only: the model is resolved at dispatch by `resolve_run_model`.
fn explicit_agent(agent_name: String, egress: TaskEgress) -> Result<String> {
    if egress.requires_local() {
        agent::egress::require_local_egress(&agent_name)?;
    }
    if egress.requires_private_network() {
        agent::egress::require_private_network_egress(&agent_name)?;
    }
    Ok(agent_name)
}

#[cfg(test)]
#[path = "run_profile_tests.rs"]
mod tests;
