// Agent advice and the removed-auto error.
// Exports: advise() and declared budget warnings.
// Deps: scoring, declared profiles, model catalog, teams, and task history.

#[path = "selection_scoring.rs"]
mod selection_scoring;
#[path = "selection_quota.rs"]
mod selection_quota;
#[path = "selection_capabilities.rs"]
mod selection_capabilities;
#[path = "selection_advice.rs"]
mod selection_advice;
#[path = "explicit_model.rs"]
mod explicit_model;
pub(crate) use explicit_model::declared_budget_warning;
pub(crate) use selection_advice::{AdviceCandidate, AdviceReport, advise, caller_advice};
pub(crate) use selection_quota::{observed_ok, quota_from, tightest_window};
use super::classifier::{self, TaskCategory};
use crate::types::AgentKind;

pub(crate) const AGENT_CAPABILITIES: &[(AgentKind, &[(TaskCategory, i32)])] =
    selection_capabilities::AGENT_CAPABILITIES;

/// Hard-error text when callers pass `auto` or leave agent empty.
pub(crate) const AUTO_AGENT_REMOVED_MSG: &str =
    "agent 'auto' was removed; declare a task profile and use `aid advise` to choose an agent";

pub(crate) fn is_removed_auto_agent(name: &str) -> bool {
    name.trim().is_empty() || name.eq_ignore_ascii_case("auto")
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod disabled_tests;
#[cfg(test)]
#[path = "selection_score_tests.rs"]
mod selection_score_tests;
#[cfg(test)]
#[path = "selection_price_tests.rs"]
mod selection_price_tests;

#[cfg(test)]
#[path = "selection_leaderboard_tests.rs"]
mod selection_leaderboard_tests;
