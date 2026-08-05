// Human and JSON output for read-only declared-profile agent advice.
// Exports: run(), build_report().
// Deps: CLI args, selection advice payload, optional read-only store, teams.

use anyhow::Result;

use crate::agent::classifier::TaskCategory;
use crate::agent::selection::{AdviceReport, advise};
use crate::cli::command_args_advise::AdviseArgs;
use crate::store::Store;
use crate::types::DeclaredTaskProfile;

pub(crate) fn run(store: Option<&Store>, args: AdviseArgs) -> Result<()> {
    let declared = DeclaredTaskProfile {
        difficulty: args.difficulty,
        budget: args.budget,
        urgency: args.urgency,
        rigor: args.rigor,
    };
    let kind_was_overridden = args.kind.is_some();
    let report = build_report(
        store,
        &args.prompt,
        declared,
        args.kind,
        args.team.as_deref(),
        args.top,
    );
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human(&report, kind_was_overridden);
    }
    Ok(())
}

pub(crate) fn build_report(
    store: Option<&Store>,
    prompt: &str,
    declared: DeclaredTaskProfile,
    kind: Option<TaskCategory>,
    team: Option<&str>,
    top: usize,
) -> AdviceReport {
    let team = team.and_then(crate::team::resolve_team);
    advise(prompt, declared, kind, team.as_ref(), store, top)
}

fn print_human(report: &AdviceReport, kind_was_overridden: bool) {
    let source = if kind_was_overridden { "declared" } else { "inferred" };
    println!(
        "Declared: {} / {} / {} / {}   (kind: {}, {})",
        report.declared.difficulty.label(),
        report.declared.budget.label(),
        report.declared.urgency.label(),
        report.declared.rigor.label(),
        report.inferred.kind.label(),
        source,
    );
    if let Some(recommended) = &report.recommended {
        // The recommendation names a route, not an agent id. `codex` alone
        // cannot say which quota pool the work will draw on, and that was the
        // question that mattered every time routing broke on 2026-08-05: an
        // exhausted route says nothing about a different provider that reaches
        // a model of the same class.
        let route = recommended_route(&recommended.agent, recommended.model.clone());
        println!(
            "Recommended: {}   score {:.1}   {}  {}",
            route,
            recommended.score,
            cost_label(recommended.est_cost_usd),
            duration_label(recommended.est_duration_secs),
        );
    } else {
        println!("Recommended: none (no installed agents)");
    }
    for (index, candidate) in report.candidates.iter().enumerate() {
        let availability = candidate_mark(candidate.installed, candidate.exclusion_reason.as_deref());
        let item = &candidate.breakdown;
        println!(
            "  {}. {:<10} {:>5.1}  base {:.1}  {:+.1} model  {:+.1} budget  {:+.1} limit  {:+.1} history  {:+.1} complexity  {:+.1} team{}",
            index + 1,
            candidate.agent,
            candidate.score,
            item.base,
            item.model_capability,
            item.budget_penalty,
            item.rate_limit_penalty,
            item.history_bonus,
            item.complexity_bonus,
            item.team_bonus,
            availability,
        );
    }
    if !report.custom_candidates.is_empty() {
        println!("Custom agents (separate capability scale):");
        for candidate in &report.custom_candidates {
            let availability = candidate_mark(candidate.installed, candidate.exclusion_reason.as_deref());
            let preference = if candidate.team_preferred { "  team preferred" } else { "" };
            println!(
                "  {:<20} capability {}  +{} strength{}{}",
                candidate.agent,
                candidate.category_capability,
                candidate.strength_bonus,
                preference,
                availability,
            );
        }
    }
    if !report.notes.is_empty() {
        println!("Notes: {}", report.notes.join("; "));
    }
}

fn candidate_mark(installed: bool, exclusion_reason: Option<&str>) -> String {
    match (installed, exclusion_reason) {
        (false, _) => " [not installed]".to_string(),
        (_, Some(reason)) => format!("  [{reason}]"),
        _ => String::new(),
    }
}

fn cost_label(cost: Option<f64>) -> String {
    cost.map(|value| format!("~${value:.2}"))
        .unwrap_or_else(|| "cost unknown".to_string())
}

fn duration_label(seconds: Option<i64>) -> String {
    seconds.map(|value| format!("~{}m", (value + 30) / 60))
        .unwrap_or_else(|| "duration unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advice_payload_round_trips_through_json() {
        let args = AdviseArgs {
            prompt: "Refactor src/main.rs".to_string(),
            difficulty: crate::types::TaskDifficulty::Complex,
            budget: crate::types::TaskBudget::Premium,
            urgency: crate::types::TaskUrgency::Urgent,
            rigor: crate::types::TaskRigor::Critical,
            kind: None,
            team: None,
            top: 5,
            json: true,
            dir: None,
        };
        let declared = DeclaredTaskProfile {
            difficulty: args.difficulty,
            budget: args.budget,
            urgency: args.urgency,
            rigor: args.rigor,
        };
        let report = advise(&args.prompt, declared, None, None, None, args.top);
        let encoded = serde_json::to_value(&report).expect("serialize advice");
        let decoded: AdviceReport = serde_json::from_value(encoded).expect("parse advice");
        assert_eq!(decoded, report);
    }

    #[test]
    fn complex_critical_surfaces_alternatives_with_shortfall_reasons() {
        let declared = DeclaredTaskProfile {
            difficulty: crate::types::TaskDifficulty::Complex,
            budget: crate::types::TaskBudget::Premium,
            urgency: crate::types::TaskUrgency::Normal,
            rigor: crate::types::TaskRigor::Critical,
        };
        let report = advise(
            "Refactor the scheduler across modules",
            declared,
            Some(crate::agent::classifier::TaskCategory::Refactoring),
            None,
            None,
            5,
        );
        assert!(report.recommended.is_some());
        let eligible = report.candidates.iter().filter(|c| c.eligible).count();
        assert!(eligible >= 2, "rigor must not collapse alternatives to one local agent");
        let cursor = report.candidates.iter().find(|c| c.agent == "cursor");
        if let Some(cursor) = cursor {
            let reason = cursor.exclusion_reason.as_deref().unwrap_or("");
            assert!(
                reason.contains("base 6 < floor 8 for complex"),
                "expected floor shortfall, got {reason:?}"
            );
        }
    }
}

/// `<cli>/<provider>/<model>` for a recommendation. A custom agent has no
/// builtin `AgentKind`, and its provider is genuinely unestablished rather than
/// absent — say `unknown` rather than dropping the dimension, so the caller can
/// see which part aid could not answer.
fn recommended_route(agent: &str, model: Option<String>) -> String {
    match crate::types::AgentKind::parse_str(agent) {
        Some(cli) if cli != crate::types::AgentKind::Custom => {
            crate::types::Route::for_cli(cli).with_model(model).id()
        }
        _ => format!("{agent}/unknown/{}", model.as_deref().unwrap_or("-")),
    }
}

#[cfg(test)]
mod recommended_route_tests {
    use super::recommended_route;

    #[test]
    fn a_builtin_agent_resolves_to_its_provider() {
        assert_eq!(
            recommended_route("codex", Some("gpt-5.6".to_string())),
            "codex/openai-chatgpt-plan/gpt-5.6"
        );
    }

    #[test]
    fn an_unpinned_model_is_shown_as_unpinned() {
        assert_eq!(recommended_route("cursor", None), "cursor/cursor-subscription/-");
    }

    /// A custom agent's provider is unestablished, not absent. Dropping the
    /// dimension would hide that aid does not know where the bill lands.
    #[test]
    fn a_custom_agent_keeps_the_dimension_and_admits_ignorance() {
        assert_eq!(recommended_route("glm5", Some("z-ai/glm5".to_string())), "glm5/unknown/z-ai/glm5");
    }
}
