// Declared-profile validation and agent/model resolution for `aid run`.
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
    AgentKind, TaskBudget, TaskDifficulty, TaskEgress, TaskRigor, TaskUrgency,
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
    store: &Arc<store::Store>, prompt: &str, dir: &Option<String>, repo: &Option<String>,
    output: &Option<String>, result_file: &Option<String>, model: &Option<String>, budget: bool,
    _difficulty: Option<TaskDifficulty>, declared_budget: Option<TaskBudget>,
    _urgency: Option<TaskUrgency>, _rigor: Option<TaskRigor>, egress: TaskEgress,
    _kind: Option<TaskCategory>, no_hint: bool, read_only: bool, sandbox: bool,
    worktree: &Option<String>, team_flag: &Option<String>, agent_name: String,
) -> Result<(String, Option<String>)> {
    if agent::selection::is_removed_auto_agent(&agent_name) {
        anyhow::bail!("{}", agent::selection::AUTO_AGENT_REMOVED_MSG);
    }
    let selection_opts = agent::RunOpts {
        dir: dir.clone().or_else(|| repo.clone())
            .or_else(|| worktree.as_ref().map(|_| ".".to_string())),
        output: output.clone(), result_file: result_file.clone(), model: model.clone(),
        budget, read_only, sandbox, context_files: vec![], session_id: None,
        env: None, env_forward: None,
    };
    let team_config = team_flag.as_deref().and_then(team::resolve_team);
    recommend_hint::emit_if_recommended(
        &agent_name, prompt, no_hint, &selection_opts, store, team_config.as_ref(),
    );
    explicit_agent(agent_name, model, declared_budget, egress)
}

fn explicit_agent(
    agent_name: String,
    model: &Option<String>,
    declared_budget: Option<TaskBudget>,
    egress: TaskEgress,
) -> Result<(String, Option<String>)> {
    if egress.requires_local() {
        agent::egress::require_local_egress(&agent_name)?;
    }
    if egress.requires_private_network() {
        agent::egress::require_private_network_egress(&agent_name)?;
    }
    let selected_kind = AgentKind::parse_str(&agent_name);
    let selected_model = if model.is_none() {
        selected_kind.and_then(|kind| agent::selection::model_for_task_budget(
            kind, declared_budget.unwrap_or(TaskBudget::Standard),
        )).map(str::to_string)
    } else {
        None
    };
    // Declared budget is a preference: never refuse dispatch when no preferred
    // tier matches. Warn and continue with whatever model we actually chose.
    if model.is_none()
        && let Some(budget) = declared_budget
        && matches!(budget, TaskBudget::Free | TaskBudget::Cheap)
    {
        let on_preference = selected_model.as_deref().is_some_and(|name| {
            selected_kind.is_some_and(|kind| {
                crate::model_catalog::model_on_budget_preference(kind, budget, name)
            })
        });
        if !on_preference {
            let chosen = selected_model.as_deref().unwrap_or("agent default");
            aid_warn!(
                "[aid] Warning: agent '{}' has no model eligible for declared budget {}; using {}",
                agent_name,
                budget.label(),
                chosen
            );
        }
    }
    Ok((agent_name, selected_model))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rigor_no_longer_gates_agent_identity() {
        let result = explicit_agent(
            "claude".into(),
            &None,
            Some(TaskBudget::Standard),
            TaskEgress::Any,
        );
        assert!(result.is_ok(), "identity gate must be gone: {result:?}");
    }

    #[test]
    fn declared_budget_is_preference_not_hard_gate() {
        // Claude has no free-tier catalog model. Pre-fix this bailed; budget
        // is a preference so dispatch must still succeed.
        let result = explicit_agent(
            "claude".into(),
            &None,
            Some(TaskBudget::Free),
            TaskEgress::Any,
        );
        assert!(
            result.is_ok(),
            "declared budget must not refuse dispatch: {result:?}"
        );
    }

    #[test]
    fn cheap_budget_dispatches_grok_with_cli_default_model() {
        let (agent, model) = explicit_agent(
            "grok".into(),
            &None,
            Some(TaskBudget::Cheap),
            TaskEgress::Any,
        )
        .expect("grok --budget cheap must dispatch");
        assert_eq!(agent, "grok");
        assert_eq!(model.as_deref(), Some("grok-4.6"));
    }

    #[test]
    fn cheap_budget_dispatches_gemini_flash_lite() {
        let (agent, model) = explicit_agent(
            "gemini".into(),
            &None,
            Some(TaskBudget::Cheap),
            TaskEgress::Any,
        )
        .expect("gemini --budget cheap must dispatch");
        assert_eq!(agent, "gemini");
        assert_eq!(model.as_deref(), Some("flash-lite"));
    }

    #[test]
    fn egress_local_refuses_builtin_third_party() {
        let err = explicit_agent(
            "codex".into(),
            &None,
            Some(TaskBudget::Standard),
            TaskEgress::Local,
        )
        .expect_err("codex must fail --egress local");
        assert!(err.to_string().contains("--egress local"));
    }

    #[test]
    fn egress_any_admits_third_party() {
        assert!(explicit_agent(
            "codex".into(),
            &None,
            Some(TaskBudget::Standard),
            TaskEgress::Any,
        )
        .is_ok());
    }

    #[test]
    fn egress_private_network_refuses_public_third_party() {
        let err = explicit_agent(
            "codex".into(),
            &None,
            Some(TaskBudget::Standard),
            TaskEgress::PrivateNetwork,
        )
        .expect_err("codex must fail --egress private-network");
        assert!(err.to_string().contains("--egress private-network"));
    }
}
