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
        let model = recommended.model.as_deref()
            .map(|value| format!("/{value}"))
            .unwrap_or_default();
        println!(
            "Recommended: {}{}   score {:.1}   {}  {}",
            recommended.agent,
            model,
            recommended.score,
            cost_label(recommended.est_cost_usd),
            duration_label(recommended.est_duration_secs),
        );
    } else {
        println!("Recommended: none (no eligible agents)");
    }
    for (index, candidate) in report.candidates.iter().enumerate() {
        let availability = match (candidate.installed, candidate.eligible) {
            (false, _) => " [not installed]",
            (_, false) => " [ineligible]",
            _ => "",
        };
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
            let availability = match (candidate.installed, candidate.eligible) {
                (false, _) => " [not installed]",
                (_, false) => " [ineligible]",
                _ => "",
            };
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
}
