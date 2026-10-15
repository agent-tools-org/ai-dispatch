// Scored API route candidates, using Claude capabilities and independent account evidence.
// No subscription quota, billing, history, or caller-pool penalty is inherited.
use super::*;
use crate::config::api_route::ApiRoute;
use crate::routing::api_budget;

pub(super) fn candidates(
    context: &CandidateContext<'_>,
    declared: DeclaredTaskProfile,
    selection: &SelectionConfig,
    store: Option<&Store>,
) -> Vec<RankedCandidate> {
    crate::config::load_config()
        .map(|config| config.api_route)
        .unwrap_or_default()
        .into_iter()
        .filter(|route| !agent_config::is_agent_disabled(&route.id))
        .map(|route| candidate(context, declared, selection, store, &route))
        .collect()
}

fn candidate(
    context: &CandidateContext<'_>,
    declared: DeclaredTaskProfile,
    selection: &SelectionConfig,
    store: Option<&Store>,
    route: &ApiRoute,
) -> RankedCandidate {
    let input = RunModelInput::declared(&route.id, AgentKind::Claude, None, declared, selection);
    let model = resolve_run_model(&input);
    let installed = crate::agent::custom_route_blocker("claude");
    let mut item = builtin_candidate(
        context,
        declared,
        None,
        model,
        (AgentKind::Custom, installed),
        false,
    );
    apply_terms(&mut item.report, context, route, store);
    apply_gates(&mut item.report, context, route, store, declared);
    let report = &item.report;
    item.order.score = report.score;
    item.order.efficiency = cost_efficiency(report.score, 0.0);
    item.order.is_default =
        context.team.and_then(|team| team.default_agent.as_deref()) == Some(route.id.as_str());
    item
}

fn apply_terms(
    report: &mut AdviceCandidate,
    context: &CandidateContext<'_>,
    route: &ApiRoute,
    store: Option<&Store>,
) {
    report.agent = route.id.clone();
    let base = context.team.and_then(|team| {
        super::super::selection_capabilities::team_override_score(team, &route.id, context.profile.category)
    });
    let base =
        base.map(f64::from)
            .or_else(|| {
                report.model.as_deref().and_then(|model| {
                    model_capability_score(AgentKind::Claude, model, context.profile.category)
                })
            })
            .unwrap_or(super::super::selection_scoring::NEUTRAL_BASE);
    report.breakdown.base = base;
    report.breakdown.history_bonus = history_bonus(store, &route.id);
    report.breakdown.headroom_penalty = 0.0;
    report.breakdown.team_bonus = if context
        .team
        .is_some_and(|team| team.preferred_agents.contains(&route.id))
    {
        3.0
    } else {
        0.0
    };
    report.quota =
        selection_quota::candidate_quota(AgentKind::Custom, Some(&route.id), report.model.as_deref());
    report.breakdown.rate_limit_penalty = if report.quota.status == "held" { -10.0 } else { 0.0 };
    report.auth = crate::routing::auth_marker::auth_status(AgentKind::Custom, Some(&route.id));
    report.capability_evidence = crate::routing::scores::evidence(
        AgentKind::Claude,
        report.model.as_deref(),
        context.profile.category,
    );
}

fn apply_gates(
    report: &mut AdviceCandidate,
    context: &CandidateContext<'_>,
    route: &ApiRoute,
    store: Option<&Store>,
    declared: DeclaredTaskProfile,
) {
    let base = report.breakdown.base;
    let mut exclusions = Exclusions::default();
    if !report.installed {
        exclusions.push("not_installed", "claude is not installed".into());
    }
    let _ = exclusions.floor(
        Some(base),
        declared.difficulty.capability_floor(),
        declared.difficulty,
        context.profile.category,
    );
    exclusions.budget(
        budget_allows(declared.budget, report.model.as_deref()),
        declared.budget,
    );
    apply_budget(report, route, store, declared, &mut exclusions);
    if report.auth.failed() {
        exclusions.push("auth_failed", "route authentication failed".into());
    }
    if report.quota.status == "held" {
        exclusions.push("needs_human", "route held; see aid agent quota".into());
    }
    report.eligible = exclusions.is_empty();
    (report.exclusion_reason, report.exclusion_codes) = exclusions.into_parts();
    report.breakdown.total = base
        + report.breakdown.history_bonus
        + report.breakdown.budget_penalty
        + report.breakdown.team_bonus
        + report.breakdown.rate_limit_penalty
        + report.breakdown.billing
        + report.breakdown.pace;
    report.score = report.breakdown.total;
}

fn history_bonus(store: Option<&Store>, id: &str) -> f64 {
    let Some(tasks) = store.and_then(|store| store.list_tasks(crate::types::TaskFilter::All).ok()) else {
        return 0.0;
    };
    let tasks: Vec<_> = tasks
        .iter()
        .filter(|task| {
            task.agent_display_name() == id
                && task.status.is_terminal()
                && task.outcome() != crate::types::TaskOutcome::Stopped
        })
        .collect();
    if tasks.len() < 5 {
        return 0.0;
    }
    let success = tasks.iter().filter(|task| task.outcome().is_success()).count();
    ((success as f64 / tasks.len() as f64 - 0.75) * 16.0)
        .round()
        .clamp(-5.0, 4.0)
}

fn apply_budget(
    report: &mut AdviceCandidate,
    route: &ApiRoute,
    store: Option<&Store>,
    declared: DeclaredTaskProfile,
    exclusions: &mut Exclusions,
) {
    match api_budget::for_route(route, store) {
        Ok(budget) => {
            if let Some(reason) = budget.billing.exclusion(declared) {
                exclusions.push("billing", reason);
            }
            report.breakdown.billing = budget.billing.penalty(declared);
            report.breakdown.pace = budget.pace;
            report.billing = Some(budget.billing);
        }
        Err(error) => exclusions.push("billing", format!("cannot read monthly spend: {error:#}")),
    }
}

#[cfg(test)]
#[path = "selection_advice_api_tests.rs"]
mod tests;
