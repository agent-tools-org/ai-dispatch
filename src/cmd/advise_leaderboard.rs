// Text leaderboard evidence alongside advised candidates.
// Exports: leaderboard_lines; no fetching or scoring.
// Deps: typed score evidence, advice payload and model labels.

use super::{AdviceReport, candidate_mark, unrated_served_line};

pub(super) fn leaderboard_lines(evidence: &crate::scores::Evidence) -> Vec<String> {
    let mut lines = vec![format!(
        "     capability: {} ({})",
        evidence
            .capability
            .map(|value| format!("{value:.2}/10"))
            .unwrap_or_else(|| "unknown".into()),
        evidence.harness
    )];
    for score in &evidence.scores {
        lines.push(format!(
            "     {} / {}: {} {} rank {}/{} effort {} cli {} date {}",
            score.source,
            score.board,
            score.value,
            score.unit,
            score
                .rank
                .map(|rank| rank.to_string())
                .unwrap_or_else(|| "unknown".into()),
            score
                .of
                .map(|of| of.to_string())
                .unwrap_or_else(|| "unknown".into()),
            score.effort.as_deref().unwrap_or("unknown"),
            score.cli.as_deref().unwrap_or("model-level"),
            score.date
        ));
    }
    lines
}

pub(super) fn print_candidates(report: &AdviceReport) {
    for (index, candidate) in report.candidates.iter().enumerate() {
        let availability = candidate_mark(
            candidate
                .exclusion_reason
                .as_deref()
                .or(candidate.demotion_reason.as_deref()),
        );
        let item = &candidate.breakdown;
        println!(
            concat!(
                "  {}. {:<10} {:>5.1}  base {:.1}  {:+.1} model  {:+.1} budget  {:+.1} limit",
                "  {:+.1} history  {:+.1} team  {:+.1} headroom  model {}{}"
            ),
            index + 1,
            candidate.agent,
            candidate.score,
            item.base,
            item.model_capability,
            item.budget_penalty,
            item.rate_limit_penalty,
            item.history_bonus,
            item.team_bonus,
            item.headroom_penalty,
            crate::agent::run_model::model_label(
                candidate.model.as_deref(),
                candidate.pinned,
                candidate.source,
            ),
            availability,
        );
        for line in leaderboard_lines(&candidate.capability_evidence) {
            println!("{line}");
        }
        if let Some(line) = unrated_served_line(candidate) {
            println!("{line}");
        }
    }
    print_custom_candidates(report);
}

fn print_custom_candidates(report: &AdviceReport) {
    if !report.custom_candidates.is_empty() {
        println!("Custom agents (separate capability scale):");
        for candidate in &report.custom_candidates {
            for line in leaderboard_lines(&candidate.capability_evidence) {
                println!("{line}");
            }
            let availability = candidate_mark(candidate.exclusion_reason.as_deref());
            let preference = if candidate.team_preferred {
                "  team preferred"
            } else {
                ""
            };
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
}
