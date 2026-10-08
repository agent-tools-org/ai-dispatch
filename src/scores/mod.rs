// Leaderboard evidence and one-source category scoring for exact price-feed IDs.
// Exports: evidence, capability_score, sources, Evidence, feed metadata types.
// Deps: price-feed alias resolution, task categories, configured CLI effort.

mod effort;
pub(crate) mod feed;
use crate::{agent::classifier::TaskCategory, types::AgentKind};
pub(crate) use feed::{Score, Source};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Evidence {
    pub canonical_id: Option<String>,
    pub scores: Vec<Score>,
    pub selected: Option<Score>,
    pub capability: Option<f64>,
    pub harness: String,
}

pub(crate) fn sources() -> BTreeMap<String, Source> {
    feed::load_cache()
        .map(|feed| feed.sources)
        .unwrap_or_default()
}

pub(crate) fn evidence(agent: AgentKind, model: Option<&str>, category: TaskCategory) -> Evidence {
    let Some(model) = model else { return unknown() };
    let Some(id) = crate::cost::canonical_feed_id(model) else {
        return unknown();
    };
    let Some(feed) = feed::load_cache() else {
        return unknown();
    };
    let effort = effort::configured(agent);
    evidence_from(&feed, &id, agent.as_str(), effort.as_deref(), category)
}

pub(crate) fn capability_score(
    agent: AgentKind,
    model: &str,
    category: TaskCategory,
) -> Option<f64> {
    evidence(agent, Some(model), category).capability
}

fn unknown() -> Evidence {
    Evidence {
        harness: "harness unmeasured".into(),
        ..Evidence::default()
    }
}

fn agentic(category: TaskCategory) -> bool {
    matches!(
        category,
        TaskCategory::SimpleEdit
            | TaskCategory::ComplexImpl
            | TaskCategory::Debugging
            | TaskCategory::Testing
            | TaskCategory::Refactoring
    )
}

fn evidence_from(
    feed: &feed::Feed,
    id: &str,
    cli: &str,
    effort: Option<&str>,
    category: TaskCategory,
) -> Evidence {
    let scores = feed
        .models
        .iter()
        .find(|model| model.id == id)
        .map(|model| model.scores.clone())
        .unwrap_or_default();
    let terminal = terminal_score(feed, &scores, cli, effort);
    // Agentic (including simple edits): exact CLI+effort Terminal-Bench 4.0, else ECI.
    // Frontend: webdev only. Research/documentation: ECI only. Never blend sources.
    let (source, board) = if category == TaskCategory::Frontend {
        ("lmarena", "webdev")
    } else {
        ("epoch", "eci")
    };
    let selected = terminal
        .filter(|_| agentic(category))
        .or_else(|| {
            scores
                .iter()
                .filter(|score| valid_score(feed, score))
                .find(|score| {
                    score.source == source
                        && score.board == board
                        && score.cli.is_none()
                        && score.effort.is_none()
                })
        })
        .cloned();
    let capability = selected.as_ref().and_then(|score| rescale(feed, score));
    Evidence {
        canonical_id: Some(id.into()),
        scores,
        selected,
        capability,
        harness: if terminal.is_some() {
            "measured"
        } else {
            "harness unmeasured"
        }
        .into(),
    }
}

fn valid_score(feed: &feed::Feed, score: &Score) -> bool {
    score.value.is_finite()
        && feed
            .sources
            .get(&score.source)
            .is_some_and(|source| source.ok)
}

fn terminal_score<'a>(
    feed: &feed::Feed,
    scores: &'a [Score],
    cli: &str,
    effort: Option<&str>,
) -> Option<&'a Score> {
    scores.iter().find(|score| {
        valid_score(feed, score)
            && score.source == "terminal_bench"
            && score.board == "terminal-bench-4.0"
            && score.cli.as_deref() == Some(cli)
            && score.effort.as_deref() == effort
    })
}

fn rescale(feed: &feed::Feed, selected: &Score) -> Option<f64> {
    // Linear 0..10 rescale: 10*(value-min)/(max-min). Terminal accuracy uses its
    // own range (0..100 percent, 0..1 rate); ECI/webdev use that source+board+unit's
    // observed model-level min/max in this snapshot. A degenerate range is unknown.
    let (min, max) = if selected.source == "terminal_bench" {
        match selected.unit.as_str() {
            "percent" => (0.0, 100.0),
            "rate" => (0.0, 1.0),
            _ => return None,
        }
    } else {
        feed.models
            .iter()
            .flat_map(|model| &model.scores)
            .filter(|score| {
                score.source == selected.source
                    && score.board == selected.board
                    && score.unit == selected.unit
                    && score.cli.is_none()
                    && score.effort.is_none()
                    && score.value.is_finite()
            })
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), score| {
                (min.min(score.value), max.max(score.value))
            })
    };
    (max > min && (min..=max).contains(&selected.value))
        .then(|| 10.0 * (selected.value - min) / (max - min))
}

#[cfg(test)]
#[path = "rule_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) mod test_support;
